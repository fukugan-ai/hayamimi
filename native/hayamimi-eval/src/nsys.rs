//! Reading the statistics `nsys stats` prints as CSV (RP-7): how many times,
//! how many bytes and how long data went between host and device, and how long
//! the kernels ran.
//!
//! `nsys` prints byte totals in megabytes with three decimals, so a total is
//! known to about a kilobyte; the megabyte is taken as 1,000,000 bytes.

use std::fmt;

use crate::error::EvalError;
use crate::gpu_results::TransferRow;
use crate::percentile::nearest_rank;

const HOST_TO_DEVICE: &str = "[CUDA memcpy Host-to-Device]";
const DEVICE_TO_HOST: &str = "[CUDA memcpy Device-to-Host]";
const BYTES_PER_MB: f64 = 1_000_000.0;

/// What one kind of copy cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemOp {
    pub count: u64,
    pub total_ns: u64,
    pub total_bytes: u64,
}

impl MemOp {
    const NONE: MemOp = MemOp {
        count: 0,
        total_ns: 0,
        total_bytes: 0,
    };
}

/// Copies between host and device in both directions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    pub host_to_device: MemOp,
    pub device_to_host: MemOp,
}

impl MemoryStats {
    /// Copies in both directions.
    pub fn count(&self) -> u64 {
        self.host_to_device.count + self.device_to_host.count
    }

    /// Time in both directions, in nanoseconds.
    pub fn total_ns(&self) -> u64 {
        self.host_to_device.total_ns + self.device_to_host.total_ns
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum NsysError {
    /// The CSV has no line that starts with the expected header.
    NoHeader(&'static str),
    /// The table has a header and no operation row at all.
    NoRows(&'static str),
    BadNumber {
        report: &'static str,
        value: String,
    },
    /// A row has fewer columns than the report defines.
    ShortRow(&'static str),
    /// The time report and the size report disagree on how many copies there were.
    CountMismatch {
        operation: &'static str,
        time_report: u64,
        size_report: u64,
    },
    /// The transfer time is over its share of the recognition time.
    OverBudget {
        transfer_us: u64,
        decode_us: u64,
    },
    /// The int8 run did not copy more often than the fp32 run.
    NotMoreTransfers {
        int8: u64,
        fp32: u64,
    },
}

impl fmt::Display for NsysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            NsysError::NoHeader(report) => write!(f, "no `{report}` table in the CSV"),
            NsysError::NoRows(report) => write!(f, "the `{report}` table has no rows"),
            NsysError::BadNumber { report, value } => {
                write!(f, "`{report}`: `{value}` is not a number")
            }
            NsysError::ShortRow(report) => write!(f, "`{report}`: a row is too short"),
            NsysError::CountMismatch {
                operation,
                time_report,
                size_report,
            } => write!(
                f,
                "{operation}: {time_report} copies in the time report, {size_report} in the size report"
            ),
            NsysError::OverBudget {
                transfer_us,
                decode_us,
            } => write!(
                f,
                "transfers took {transfer_us} us against {decode_us} us of recognition, over 20%"
            ),
            NsysError::NotMoreTransfers { int8, fp32 } => {
                write!(f, "int8 copied {int8} times, fp32 {fp32}: not more")
            }
        }
    }
}

impl std::error::Error for NsysError {}

/// The data rows of the table whose header line starts with `header`.
fn table_rows<'a>(csv: &'a str, header: &str) -> Option<Vec<&'a str>> {
    let mut lines = csv.lines().skip_while(|line| !line.starts_with(header));
    lines.next()?;
    Some(
        lines
            .take_while(|line| !line.trim().is_empty())
            .collect::<Vec<_>>(),
    )
}

fn float(report: &'static str, text: &str) -> Result<f64, NsysError> {
    text.trim()
        .parse::<f64>()
        .map_err(|_| NsysError::BadNumber {
            report,
            value: text.to_owned(),
        })
}

fn whole(report: &'static str, text: &str) -> Result<u64, NsysError> {
    text.trim()
        .parse::<u64>()
        .map_err(|_| NsysError::BadNumber {
            report,
            value: text.to_owned(),
        })
}

/// The row of `operation` as its numeric columns; the operation name is the last column.
fn operation_row<'a>(rows: &[&'a str], operation: &str) -> Option<Vec<&'a str>> {
    rows.iter().find_map(|row| {
        let (numbers, name) = row.rsplit_once(',')?;
        (name.trim() == operation).then(|| numbers.split(',').collect())
    })
}

fn one_operation(
    time_rows: &[&str],
    size_rows: &[&str],
    operation: &'static str,
) -> Result<MemOp, NsysError> {
    let time = operation_row(time_rows, operation);
    let size = operation_row(size_rows, operation);
    let (time, size) = match (time, size) {
        (Some(time), Some(size)) => (time, size),
        // DEFAULT: nsys prints a row only for an operation that happened, so a row missing from both reports is zero copies.
        (None, None) => return Ok(MemOp::NONE),
        _ => {
            return Err(NsysError::ShortRow(
                "one report lacks the operation the other has",
            ));
        }
    };
    let (Some(total_ns), Some(time_count)) = (time.get(1), time.get(2)) else {
        return Err(NsysError::ShortRow("cuda_gpu_mem_time_sum"));
    };
    let (Some(total_mb), Some(size_count)) = (size.first(), size.get(1)) else {
        return Err(NsysError::ShortRow("cuda_gpu_mem_size_sum"));
    };
    let time_count = whole("cuda_gpu_mem_time_sum", time_count)?;
    let size_count = whole("cuda_gpu_mem_size_sum", size_count)?;
    if time_count != size_count {
        return Err(NsysError::CountMismatch {
            operation,
            time_report: time_count,
            size_report: size_count,
        });
    }
    let megabytes = float("cuda_gpu_mem_size_sum", total_mb)?;
    // A rounded, non-negative megabyte total times 1e6 fits u64 for any real capture.
    let total_bytes = (megabytes * BYTES_PER_MB).round() as u64;
    Ok(MemOp {
        count: time_count,
        total_ns: whole("cuda_gpu_mem_time_sum", total_ns)?,
        total_bytes,
    })
}

/// Host → device and device → host from the `cuda_gpu_mem_time_sum` and
/// `cuda_gpu_mem_size_sum` CSV of one profile.
pub fn parse_memory_stats(time_csv: &str, size_csv: &str) -> Result<MemoryStats, NsysError> {
    let time_rows =
        table_rows(time_csv, "Time (%)").ok_or(NsysError::NoHeader("cuda_gpu_mem_time_sum"))?;
    let size_rows =
        table_rows(size_csv, "Total (MB)").ok_or(NsysError::NoHeader("cuda_gpu_mem_size_sum"))?;
    if time_rows.is_empty() {
        return Err(NsysError::NoRows("cuda_gpu_mem_time_sum"));
    }
    if size_rows.is_empty() {
        return Err(NsysError::NoRows("cuda_gpu_mem_size_sum"));
    }
    Ok(MemoryStats {
        host_to_device: one_operation(&time_rows, &size_rows, HOST_TO_DEVICE)?,
        device_to_host: one_operation(&time_rows, &size_rows, DEVICE_TO_HOST)?,
    })
}

/// The kernel time of one profile in nanoseconds: the sum of `Total Time (ns)`
/// over the rows of the `cuda_gpu_kern_sum` CSV.
pub fn parse_kernel_time_ns(kern_csv: &str) -> Result<u64, NsysError> {
    let rows = table_rows(kern_csv, "Time (%),Total Time (ns),Instances")
        .ok_or(NsysError::NoHeader("cuda_gpu_kern_sum"))?;
    if rows.is_empty() {
        return Err(NsysError::NoRows("cuda_gpu_kern_sum"));
    }
    let mut total = 0_u64;
    for row in rows {
        // Kernel names hold commas, so only the leading columns are split off.
        let mut columns = row.splitn(3, ',');
        let (Some(_), Some(nanoseconds)) = (columns.next(), columns.next()) else {
            return Err(NsysError::ShortRow("cuda_gpu_kern_sum"));
        };
        total += whole("cuda_gpu_kern_sum", nanoseconds)?;
    }
    Ok(total)
}

/// Takes one direction out of a profile's statistics.
type PickDirection = fn(&MemoryStats) -> MemOp;

fn percentiles(values: &[u64]) -> Result<(u64, u64), EvalError> {
    let signed: Vec<i64> = values
        .iter()
        .map(|v| {
            i64::try_from(*v)
                .map_err(|_| EvalError::Invalid(String::from("a value does not fit i64")))
        })
        .collect::<Result<_, _>>()?;
    let at = |p: usize| -> Result<u64, EvalError> {
        u64::try_from(nearest_rank(&signed, p)?)
            .map_err(|_| EvalError::Invalid(String::from("a percentile is negative")))
    };
    Ok((at(50)?, at(95)?))
}

/// The p50 and p95 per utterance of the copies in each direction, from one profile per utterance.
/// Times are in microseconds, rounded up.
pub fn transfer_rows(
    variant: &str,
    per_utterance: &[MemoryStats],
) -> Result<Vec<TransferRow>, EvalError> {
    if per_utterance.is_empty() {
        return Err(EvalError::Invalid(String::from("no profiles to summarise")));
    }
    let directions: [(&str, PickDirection); 2] = [
        ("host→device", |s| s.host_to_device),
        ("device→host", |s| s.device_to_host),
    ];
    directions
        .into_iter()
        .map(|(direction, pick)| {
            let ops: Vec<MemOp> = per_utterance.iter().map(pick).collect();
            let column = |value: fn(&MemOp) -> u64| -> Result<(u64, u64), EvalError> {
                percentiles(&ops.iter().map(value).collect::<Vec<_>>())
            };
            let (count_p50, count_p95) = column(|op| op.count)?;
            let (bytes_p50, bytes_p95) = column(|op| op.total_bytes)?;
            let (ns_p50, ns_p95) = column(|op| op.total_ns)?;
            Ok(TransferRow {
                direction: direction.to_owned(),
                variant: variant.to_owned(),
                count_p50,
                count_p95,
                bytes_p50,
                bytes_p95,
                time_us_p50: ns_p50.div_ceil(1000),
                time_us_p95: ns_p95.div_ceil(1000),
            })
        })
        .collect()
}

/// RP-7: the transfer time p95 is at most 20% of the recognition time p95 (both in microseconds).
pub fn check_transfer_budget(transfer_p95_us: u64, decode_p95_us: u64) -> Result<(), NsysError> {
    if transfer_p95_us * 100 <= decode_p95_us * 20 {
        return Ok(());
    }
    Err(NsysError::OverBudget {
        transfer_us: transfer_p95_us,
        decode_us: decode_p95_us,
    })
}

/// RP-7 (negative control): a run that loads an int8 model on CUDA copies more often than the fp32 run.
pub fn check_int8_copies_more(int8: &MemoryStats, fp32: &MemoryStats) -> Result<(), NsysError> {
    if int8.count() > fp32.count() {
        return Ok(());
    }
    Err(NsysError::NotMoreTransfers {
        int8: int8.count(),
        fp32: fp32.count(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TIME: &str = "Processing [x.sqlite] with [cuda_gpu_mem_time_sum.py]...\nTime (%),Total Time (ns),Count,Avg (ns),Med (ns),Min (ns),Max (ns),StdDev (ns),Operation\n98.6,38125806,1326,28752.5,352.0,224,634928,83651.6,[CUDA memcpy Host-to-Device]\n0.7,282780,289,978.5,512.0,384,66942,3924.2,[CUDA memcpy Device-to-Host]\n0.6,232731,402,578.9,544.0,512,1920,154.9,[CUDA memcpy Device-to-Device]\n";
    const SIZE: &str = "Total (MB),Count,Avg (MB),Med (MB),Min (MB),Max (MB),StdDev (MB),Operation\n269.858,1326,0.204,0.002,0.000,4.096,0.587,[CUDA memcpy Host-to-Device]\n15.277,402,0.038,0.008,0.000,0.975,0.114,[CUDA memcpy Device-to-Device]\n2.488,289,0.009,0.008,0.000,0.487,0.028,[CUDA memcpy Device-to-Host]\n";
    const KERN: &str = "Time (%),Total Time (ns),Instances,Avg (ns),Med (ns),Min (ns),Max (ns),StdDev (ns),Name\n50.0,1000,2,500.0,500.0,400,600,10.0,\"void a<(int)1, float>(int)\"\n50.0,500,1,500.0,500.0,500,500,0.0,b\n";

    #[test]
    fn both_directions_are_read_with_count_time_and_bytes() -> Result<(), NsysError> {
        let stats = parse_memory_stats(TIME, SIZE)?;
        assert_eq!(stats.host_to_device.count, 1326);
        assert_eq!(stats.host_to_device.total_ns, 38_125_806);
        assert_eq!(stats.host_to_device.total_bytes, 269_858_000);
        assert_eq!(stats.device_to_host.count, 289);
        assert_eq!(stats.device_to_host.total_bytes, 2_488_000);
        assert_eq!(stats.count(), 1615);
        assert_eq!(stats.total_ns(), 38_408_586);
        Ok(())
    }

    #[test]
    fn kernel_time_is_the_sum_even_when_a_name_holds_commas() -> Result<(), NsysError> {
        assert_eq!(parse_kernel_time_ns(KERN)?, 1500);
        Ok(())
    }

    #[test]
    fn a_csv_without_the_table_is_an_error_not_zero() {
        assert_eq!(
            parse_memory_stats("nothing", SIZE),
            Err(NsysError::NoHeader("cuda_gpu_mem_time_sum"))
        );
        assert_eq!(
            parse_kernel_time_ns("nothing"),
            Err(NsysError::NoHeader("cuda_gpu_kern_sum"))
        );
    }

    #[test]
    fn a_table_without_rows_is_an_error() {
        let time = "Time (%),Total Time (ns),Count\n\n";
        assert_eq!(
            parse_memory_stats(time, SIZE),
            Err(NsysError::NoRows("cuda_gpu_mem_time_sum"))
        );
    }

    #[test]
    fn a_missing_direction_is_zero_copies_when_the_table_has_other_rows() -> Result<(), NsysError> {
        let time = "Time (%),Total Time (ns),Count,Avg (ns),Med (ns),Min (ns),Max (ns),StdDev (ns),Operation\n100.0,10,1,10.0,10.0,10,10,0.0,[CUDA memcpy Host-to-Device]\n";
        let size = "Total (MB),Count,Avg (MB),Med (MB),Min (MB),Max (MB),StdDev (MB),Operation\n0.001,1,0.001,0.001,0.001,0.001,0.0,[CUDA memcpy Host-to-Device]\n";
        let stats = parse_memory_stats(time, size)?;
        assert_eq!(stats.device_to_host, MemOp::NONE);
        assert_eq!(stats.host_to_device.total_bytes, 1000);
        Ok(())
    }

    #[test]
    fn disagreeing_counts_are_an_error() {
        let size = SIZE.replace("1326", "1327");
        assert!(matches!(
            parse_memory_stats(TIME, &size),
            Err(NsysError::CountMismatch { .. })
        ));
    }

    #[test]
    fn a_number_that_is_not_a_number_is_an_error() {
        let time = TIME.replace("38125806", "many");
        assert!(matches!(
            parse_memory_stats(&time, SIZE),
            Err(NsysError::BadNumber { .. })
        ));
    }

    fn stats(count: u64, bytes: u64, ns: u64) -> MemoryStats {
        let op = MemOp {
            count,
            total_ns: ns,
            total_bytes: bytes,
        };
        MemoryStats {
            host_to_device: op,
            device_to_host: MemOp {
                count: count + 1,
                ..op
            },
        }
    }

    #[test]
    fn transfer_rows_take_percentiles_per_direction_and_round_time_up() -> Result<(), EvalError> {
        let profiles: Vec<MemoryStats> =
            (1..=20).map(|n| stats(n, n * 100, n * 1000 + 1)).collect();
        let rows = transfer_rows("fp32", &profiles)?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].direction, "host→device");
        assert_eq!((rows[0].count_p50, rows[0].count_p95), (10, 19));
        assert_eq!((rows[0].bytes_p50, rows[0].bytes_p95), (1000, 1900));
        assert_eq!((rows[0].time_us_p50, rows[0].time_us_p95), (11, 20));
        assert_eq!(rows[1].direction, "device→host");
        assert_eq!(rows[1].count_p50, 11);
        Ok(())
    }

    #[test]
    fn no_profiles_is_an_error() {
        assert!(transfer_rows("fp32", &[]).is_err());
    }

    #[test]
    fn the_budget_is_twenty_percent_inclusive() {
        assert_eq!(check_transfer_budget(2000, 10_000), Ok(()));
        assert!(matches!(
            check_transfer_budget(2001, 10_000),
            Err(NsysError::OverBudget { .. })
        ));
        assert!(check_transfer_budget(1, 0).is_err());
        assert_eq!(check_transfer_budget(0, 0), Ok(()));
    }

    #[test]
    fn int8_must_copy_more_than_fp32() -> Result<(), NsysError> {
        let many = parse_memory_stats(TIME, SIZE)?;
        let few = MemoryStats {
            host_to_device: MemOp {
                count: 10,
                ..many.host_to_device
            },
            device_to_host: MemOp {
                count: 10,
                ..many.device_to_host
            },
        };
        assert_eq!(check_int8_copies_more(&many, &few), Ok(()));
        assert!(check_int8_copies_more(&few, &many).is_err());
        assert!(check_int8_copies_more(&few, &few).is_err());
        Ok(())
    }
}
