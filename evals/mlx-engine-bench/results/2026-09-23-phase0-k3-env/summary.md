# phase0-k3-env

engine `mihai-qwen3.8-27b-atlassian-q8-mlx` · sampling {'temperature': 0.0} · reps 3 · nonce `phase0-k3-env-20260923T165556`

median (min–max) over reps

| workload | TTFT s | prefill tok/s (uncached) | decode tok/s | prompt tok | completion tok | cache tokens_saved | MTP accepted/drafted (verify calls) |
|---|---|---|---|---|---|---|---|
| a | 1.06 (1.00–1.21) | 199 (174–210) | 22.8 (20.5–22.9) | 210 (210–210) | 300 (300–300) | 0 (0–0) | 477/1118 (407) |
| b | 185.36 (180.21–189.90) | 173 (168–178) | 17.1 (15.9–17.7) | 31989 (31989–31989) | 189 (189–193) | 0 (0–0) | 229/383 (337) |
