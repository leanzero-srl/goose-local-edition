# phase0-k2

engine `mihai-qwen3.8-27b-atlassian-q8-mlx` · sampling {'temperature': 0.0} · reps 3 · nonce `phase0-k2-20260923T170654`

median (min–max) over reps

| workload | TTFT s | prefill tok/s (uncached) | decode tok/s | prompt tok | completion tok | cache tokens_saved | MTP accepted/drafted (verify calls) |
|---|---|---|---|---|---|---|---|
| a | 1.33 (1.17–1.39) | 157 (150–179) | 23.0 (18.9–23.6) | 209 (209–209) | 300 (300–300) | 0 (0–0) | 448/852 (439) |
| b | 171.12 (165.90–174.82) | 187 (183–193) | 17.4 (17.0–18.3) | 31988 (31988–31988) | 189 (187–189) | 0 (0–0) | 233/396 (317) |
