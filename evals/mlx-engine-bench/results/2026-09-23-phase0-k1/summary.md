# phase0-k1

engine `mihai-qwen3.8-27b-atlassian-q8-mlx` · sampling {'temperature': 0.0} · reps 3 · nonce `phase0-k1-20260923T171706`

median (min–max) over reps

| workload | TTFT s | prefill tok/s (uncached) | decode tok/s | prompt tok | completion tok | cache tokens_saved | MTP accepted/drafted (verify calls) |
|---|---|---|---|---|---|---|---|
| a | 1.40 (1.08–1.45) | 149 (144–193) | 20.2 (20.0–20.2) | 209 (209–209) | 300 (300–300) | 0 (0–0) | 349/538 (538) |
| b | 169.83 (169.49–172.28) | 188 (186–189) | 18.4 (18.2–18.8) | 31988 (31988–31988) | 191 (189–200) | 0 (0–0) | 231/344 (344) |
