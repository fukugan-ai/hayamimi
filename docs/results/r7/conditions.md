## 測定時の条件

- 機械：RTX 5060 Ti 16 GB。ほかの席の仕事と同居していた。load average（1 分）は、CPU の run の間は 16 から 32、CUDA の run の間は 10 から 25（`uptime`）。
- CUDA の run の前後とも、`nvidia-smi --query-compute-apps` にほかの 3 process が居た（WSL では使用量が N/A）。
- CPU の run と CUDA の run は、同じ時間帯に続けては取っていない。認識の置き場の判定（CPU が GPU より遅い）は、この負荷の下の値であり、取り直すまで確定とは読まない（NOT VERIFIED）。
- CUDA では `recognize` と `encoder_only,search_replay` を別 process で取った。同じ process では ort が DefaultLogger の error で落ちる。
- 探索の内訳は、ort で joiner と decoder を呼んだ時間である。sherpa-onnx の内部の時間ではなく、H31 の原因は未特定。
