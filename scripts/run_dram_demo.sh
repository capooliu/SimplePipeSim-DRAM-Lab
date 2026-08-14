#!/usr/bin/env bash

# Source this file once, then invoke run_qsort or run_workload interactively.

DRAM_DEMO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DRAM_DEMO_TARGET_DIR="${CARGO_TARGET_DIR:-${HOME}/.cache/simplepipesim-target}"
DRAM_DEMO_ELF_DIR="${ELF_DIR:-${DRAM_DEMO_TARGET_DIR}/riscv32im-unknown-none-elf/debug}"
DRAM_DEMO_RESULTS_DIR="${RESULTS_DIR:-${DRAM_DEMO_ROOT}/results}"

export CARGO_TARGET_DIR="${DRAM_DEMO_TARGET_DIR}"

run_dram_case() {
    local prog="$1"
    local output_name="$2"
    local trcd="$3"
    local tcl="$4"
    local trp="$5"
    local output_dir="$6"
    local output_path="${output_dir}/${output_name}.json"

    mkdir -p "${output_dir}"
    echo "=== ${output_name}: tRCD=${trcd}, tCL=${tcl}, tRP=${trp} ==="

    (
        cd "${DRAM_DEMO_ROOT}"
        RUSTFLAGS="-Awarnings" cargo run --quiet --release -p simulator -- \
            --prog "${prog}" \
            --elf-dir "${DRAM_DEMO_ELF_DIR}" \
            --memory dram \
            --dram-trcd "${trcd}" \
            --dram-tcl "${tcl}" \
            --dram-trp "${trp}" \
            --stats-out "${output_path}"
    )

    grep -E 'total_ticked_cycle|row_buffer_hit_cnt|row_buffer_miss_cnt' "${output_path}"
}

run_qsort() {
    local name="$1"
    local trcd="$2"
    local tcl="$3"
    local trp="$4"

    run_dram_case qsort "qsort_${name}" "${trcd}" "${tcl}" "${trp}" \
        "${DRAM_DEMO_RESULTS_DIR}/timing_sweep"
}

run_workload() {
    local prog="$1"

    run_dram_case "${prog}" "${prog}_dram" 4 4 4 \
        "${DRAM_DEMO_RESULTS_DIR}/dram_workload_comparison"
}

echo "DRAM demo commands loaded:"
echo "  run_qsort <name> <tRCD> <tCL> <tRP>"
echo "  run_workload <ELF>"
