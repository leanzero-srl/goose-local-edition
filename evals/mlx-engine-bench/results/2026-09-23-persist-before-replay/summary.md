# persist-before-replay

engine `mihai-qwen3.8-27b-atlassian-q8-mlx` · sampling {'temperature': 0.0} · reps 1 · nonce `baseline-v0.14.3-lz.1-20260923T162113`

median (min–max) over reps

| workload | TTFT s | prefill tok/s (uncached) | decode tok/s | prompt tok | completion tok | cache tokens_saved | MTP accepted/drafted (verify calls) |
|---|---|---|---|---|---|---|---|
| c turn 1 | 232.89 (232.89–232.89) | 181 (181–181) | 23.4 (23.4–23.4) | 42238 (42238–42238) | 39 (39–39) | 0 (0–0) | 15/22 (16) |
| c turn 2 | 26.00 (26.00–26.00) | 140 (140–140) | 23.3 (23.3–23.3) | 44601 (44601–44601) | 41 (41–41) | 40960 (40960–40960) | 19/35 (16) |
| c turn 3 | 30.65 (30.65–30.65) | 126 (126–126) | 16.7 (16.7–16.7) | 46879 (46879–46879) | 40 (40–40) | 43008 (43008–43008) | 13/35 (14) |
| c turn 4 | 34.73 (34.73–34.73) | 121 (121–121) | 20.2 (20.2–20.2) | 49260 (49260–49260) | 45 (45–45) | 45056 (45056–45056) | 8/27 (9) |
