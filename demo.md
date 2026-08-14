# DRAM Lab Demo

## 進入專案

從 Windows Terminal／PowerShell 開始：

```bash
wsl
cd <SimplePipeSim clone 的位置>
source scripts/run_dram_demo.sh
```

如果專案放在 Windows C 槽，路徑會是 `/mnt/c/.../SimplePipeSim`；如果直接 clone 在 WSL，通常是 `~/SimplePipeSim`。

執行 `source` 後會看到：

```text
DRAM demo commands loaded:
  run_qsort <name> <tRCD> <tCL> <tRP>
  run_workload <ELF>
```

## Demo 1：qsort Timing Sweep

現場逐行輸入：

```bash
run_qsort baseline 4 4 4
run_qsort trcd12 12 4 4
run_qsort tcl12 4 12 4
run_qsort trp12 4 4 12
```

每次執行會先顯示本次設定，再顯示 Pipeline、Cache 與 DRAM 統計。以 Baseline 為例：

```text
=== qsort_baseline: tRCD=4, tCL=4, tRP=4 ===
Program: qsort
Replacement policy: Fifo
Branch predictor: Bimodal
Backing memory: Dram
Pipeline: cycles=123024 retired=63871 ipc=0.5192 branch_miss=0.2094
  L1-I$ load_cnt=94455 load_miss=4279 ...
  L1-D$ load_cnt=12960 load_miss=1039 ... store_miss=1623 ...
  L2$   load_cnt=7313 load_miss=372 ... store_miss=2 ...
wrote stats JSON to .../results/timing_sweep/qsort_baseline.json
    "total_ticked_cycle": 123024,
    "dram_access_cnt": 385,
    "cold_open_cnt": 1,
    "row_buffer_hit_cnt": 148,
    "row_buffer_miss_cnt": 237,
    "row_conflict_cnt": 236,
    "total_access_time_cycles": 3432,
    "average_access_time_cycles": 8.914285714285715
```

四個指令最後會顯示的關鍵結果：

| 指令 | total_ticked_cycle | Row hit | Row miss | JSON |
|---|---:|---:|---:|---|
| `run_qsort baseline 4 4 4` | 123,024 | 148 | 237 | `qsort_baseline.json` |
| `run_qsort trcd12 12 4 4` | 124,920 | 148 | 237 | `qsort_trcd12.json` |
| `run_qsort tcl12 4 12 4` | 126,104 | 148 | 237 | `qsort_tcl12.json` |
| `run_qsort trp12 4 4 12` | 124,912 | 148 | 237 | `qsort_trp12.json` |

結果位置：

```text
results/timing_sweep/qsort_baseline.json
results/timing_sweep/qsort_trcd12.json
results/timing_sweep/qsort_tcl12.json
results/timing_sweep/qsort_trp12.json
```

| Case | tRCD | tCL | tRP | DRAM access cycles | Workload total cycles | 增加 cycles |
|---|---:|---:|---:|---:|---:|---:|
| Baseline | 4 | 4 | 4 | 3,432 | 123,024 | 0 |
| tRCD high | 12 | 4 | 4 | 5,328 | 124,920 | +1,896 |
| tCL high | 4 | 12 | 4 | 6,512 | 126,104 | +3,080 |
| tRP high | 4 | 4 | 12 | 5,320 | 124,912 | +1,888 |

四組共同的 access 組成：

```text
Row hit       = 148
Row miss      = 237
DRAM requests = 385
Cold open     = 1
Row conflict  = 236
```

## Demo 2：不同 ELF 的 DRAM Traffic

六個 workload 已跑完；要重跑時逐行輸入：

```bash
run_workload hello
run_workload print_nums
run_workload factorial
run_workload qsort
run_workload msort
run_workload matmul
```

每個指令的輸出格式相同。以 `run_workload qsort` 為例：

```text
=== qsort_dram: tRCD=4, tCL=4, tRP=4 ===
Program: qsort
Replacement policy: Fifo
Branch predictor: Bimodal
Backing memory: Dram
Pipeline: cycles=123024 retired=63871 ipc=0.5192 branch_miss=0.2094
  L1-I$ load_cnt=94455 load_miss=4279 ...
  L1-D$ load_cnt=12960 load_miss=1039 ... store_miss=1623 ...
  L2$   load_cnt=7313 load_miss=372 ... store_miss=2 ...
wrote stats JSON to .../results/dram_workload_comparison/qsort_dram.json
    "total_ticked_cycle": 123024,
    "dram_access_cnt": 385,
    "cold_open_cnt": 1,
    "row_buffer_hit_cnt": 148,
    "row_buffer_miss_cnt": 237,
    "row_conflict_cnt": 236,
    "total_access_time_cycles": 3432,
    "average_access_time_cycles": 8.914285714285715
```

結果位置：

```text
results/dram_workload_comparison/<ELF>_dram.json
```

目前共有六個 JSON：

```text
hello_dram.json
print_nums_dram.json
factorial_dram.json
qsort_dram.json
msort_dram.json
matmul_dram.json
```

| ELF | L2 miss 次數 | DRAM access 次數 | Row hit | Row miss | DRAM 總 access time | 平均 access time |
|---|---:|---:|---:|---:|---:|---:|
| `hello` | 64 | 64 | 21 | 43 | 596 cycles | 9.31 cycles |
| `print_nums` | 178 | 178 | 74 | 104 | 1,540 cycles | 8.65 cycles |
| `factorial` | 90 | 90 | 41 | 49 | 748 cycles | 8.31 cycles |
| `qsort` | 374 | 385 | 148 | 237 | 3,432 cycles | 8.91 cycles |
| `msort` | 308 | 324 | 134 | 190 | 2,812 cycles | 8.68 cycles |
| `matmul` | 2,049 | 2,184 | 260 | 1,924 | 24,124 cycles | 11.05 cycles |

欄位算法：

```text
L2 miss = l2.load_miss_cnt + l2.store_miss_cnt
DRAM access 次數 = backing_memory.dram_access_cnt
DRAM 總 access time = backing_memory.total_access_time_cycles
平均 access time = backing_memory.average_access_time_cycles
```

## 查看 JSON 的重要結果

```bash
grep -H -E 'total_ticked_cycle|load_miss_cnt|store_miss_cnt|dram_access_cnt|cold_open_cnt|row_buffer_hit_cnt|row_buffer_miss_cnt|row_conflict_cnt|total_access_time_cycles|average_access_time_cycles|dram_trcd|dram_tcl|dram_trp' \
  results/dram_workload_comparison/*.json
```
