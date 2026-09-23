# persist-after-replay

engine `mihai-qwen3.8-27b-atlassian-q8-mlx` · sampling {'temperature': 0.0} · reps 1 · nonce `phase0-final-20260923T172840`

median (min–max) over reps

| workload | TTFT s | prefill tok/s (uncached) | decode tok/s | prompt tok | completion tok | cache tokens_saved | MTP accepted/drafted (verify calls) |
|---|---|---|---|---|---|---|---|
| c turn 1 | 206.08 (206.08–206.08) | 205 (205–205) | 24.1 (24.1–24.1) | 42229 (42229–42229) | 39 (39–39) | 0 (0–0) | 15/22 (16) |
| c turn 2 | 239.06 (239.06–239.06) | 187 (187–187) | 26.1 (26.1–26.1) | 44592 (44592–44592) | 34 (34–34) | 0 (0–0) | 16/29 (14) |
| c turn 3 | 25.32 (25.32–25.32) | 153 (153–153) | 23.8 (23.8–23.8) | 46870 (46870–46870) | 40 (40–40) | 43008 (43008–43008) | 16/39 (13) |
| c turn 4 | 28.20 (28.20–28.20) | 149 (149–149) | 25.6 (25.6–25.6) | 49251 (49251–49251) | 45 (45–45) | 45056 (45056–45056) | 7/22 (8) |
