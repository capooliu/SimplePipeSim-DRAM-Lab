use clap::Parser;
use std::path::PathBuf;

/// SimplePipeSim — a teaching RISC-V pipeline simulator focused on the
/// cache subsystem. Configure the three cache levels and the replacement
/// policy from the command line, then either print stats to stdout or
/// emit JSON for offline plotting.
#[derive(Parser, Debug, Clone, serde::Serialize)]
#[command(name = "simulator", about, long_about = None)]
pub struct Args {
    /// Name of the runtime binary to run (e.g. "matmul", "qsort", "hello").
    /// Looked up relative to `--elf-dir`.
    #[arg(long)]
    pub prog: String,

    /// Directory containing pre-built RISC-V ELF binaries.
    #[arg(long, default_value = "../target/riscv32im-unknown-none-elf/debug")]
    pub elf_dir: PathBuf,

    // -------- L1 Instruction cache --------
    #[arg(long, default_value_t = 2048)]
    pub l1i_size: usize,
    #[arg(long, default_value_t = 32)]
    pub l1i_block: usize,
    #[arg(long, default_value_t = 4)]
    pub l1i_ways: usize,
    #[arg(long, default_value_t = 2)]
    pub l1i_penalty: usize,

    // -------- L1 Data cache --------
    #[arg(long, default_value_t = 256)]
    pub l1d_size: usize,
    #[arg(long, default_value_t = 32)]
    pub l1d_block: usize,
    #[arg(long, default_value_t = 2)]
    pub l1d_ways: usize,
    #[arg(long, default_value_t = 2)]
    pub l1d_penalty: usize,

    // -------- L2 unified cache --------
    #[arg(long, default_value_t = 16384)]
    pub l2_size: usize,
    #[arg(long, default_value_t = 64)]
    pub l2_block: usize,
    #[arg(long, default_value_t = 4)]
    pub l2_ways: usize,
    #[arg(long, default_value_t = 10)]
    pub l2_penalty: usize,

    /// Replacement policy for all caches.
    #[arg(long, value_enum, default_value_t = ReplacementPolicyArg::Fifo)]
    pub rp: ReplacementPolicyArg,

    /// Write policy for all caches.
    #[arg(long, value_enum, default_value_t = WritePolicyArg::WbWa)]
    pub wp: WritePolicyArg,

    /// Hardware prefetcher for all caches.
    #[arg(long, value_enum, default_value_t = PrefetcherArg::Null)]
    pub prefetcher: PrefetcherArg,

    /// Branch predictor selected for control-flow instructions.
    #[arg(long, value_enum, default_value_t = BranchPredictorArg::Bimodal)]
    pub bp: BranchPredictorArg,

    /// Backing memory behind the unified L2 cache.
    #[arg(long, value_enum, default_value_t = BackingMemoryArg::SimpleMem)]
    pub memory: BackingMemoryArg,

    // -------- Single-bank DRAM timing --------
    /// DRAM row-to-column delay in cycles. Only used with `--memory dram`.
    #[arg(long, default_value_t = 4, value_parser = parse_positive_usize)]
    pub dram_trcd: usize,

    /// DRAM CAS latency in cycles. Only used with `--memory dram`.
    #[arg(long, default_value_t = 4, value_parser = parse_positive_usize)]
    pub dram_tcl: usize,

    /// DRAM precharge time in cycles. Only used with `--memory dram`.
    #[arg(long, default_value_t = 4, value_parser = parse_positive_usize)]
    pub dram_trp: usize,

    /// Write per-cache statistics to this JSON file.
    /// Schema: { "pipeline": StatisticInfo, "l1i": StatisticInfo,
    /// "l1d": StatisticInfo, "l2": StatisticInfo, "backing_memory": ...,
    /// "final_registers": [...], "config": { mirror of CLI args } }.
    #[arg(long)]
    pub stats_out: Option<PathBuf>,
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| format!("expected a positive integer, got {value:?}"))?;
    if parsed == 0 {
        Err("DRAM timing must be at least 1 cycle".to_string())
    } else {
        Ok(parsed)
    }
}

impl Args {
    /// Build the backing-memory configuration selected by the CLI.
    /// DRAM timing flags are intentionally ignored for `simple-mem`.
    pub fn backing_memory_kind(&self) -> crate::sim::runner::BackingMemoryKind {
        use crate::hardware::mem::simple_dram::DramTiming;
        use crate::sim::runner::BackingMemoryKind;

        match self.memory {
            BackingMemoryArg::SimpleMem => BackingMemoryKind::SimpleMem,
            BackingMemoryArg::Dram => BackingMemoryKind::SimpleDram {
                timing: DramTiming {
                    t_rcd: self.dram_trcd,
                    t_cl: self.dram_tcl,
                    t_rp: self.dram_trp,
                },
            },
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReplacementPolicyArg {
    Fifo,
    Random,
    /// Tree-based pseudo-LRU. Requires power-of-two associativity.
    Plru,
}

impl From<ReplacementPolicyArg> for crate::sim::runner::ReplacementPolicyKind {
    fn from(a: ReplacementPolicyArg) -> Self {
        use crate::sim::runner::ReplacementPolicyKind;
        match a {
            ReplacementPolicyArg::Fifo => ReplacementPolicyKind::Fifo,
            ReplacementPolicyArg::Random => ReplacementPolicyKind::Random,
            ReplacementPolicyArg::Plru => ReplacementPolicyKind::Plru,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum WritePolicyArg {
    /// Write-back + write-allocate (default).
    #[clap(name = "wb-wa")]
    #[serde(rename = "wb-wa")]
    WbWa,
    /// Write-back + no-write-allocate.
    #[clap(name = "wb-nwa")]
    #[serde(rename = "wb-nwa")]
    WbNwa,
    /// Write-through + write-allocate.
    #[clap(name = "wt-wa")]
    #[serde(rename = "wt-wa")]
    WtWa,
    /// Write-through + no-write-allocate.
    #[clap(name = "wt-nwa")]
    #[serde(rename = "wt-nwa")]
    WtNwa,
}

impl From<WritePolicyArg> for crate::hardware::mem::general_cache::write_policy::WritePolicy {
    fn from(a: WritePolicyArg) -> Self {
        use crate::hardware::mem::general_cache::write_policy::WritePolicy;
        match a {
            WritePolicyArg::WbWa => WritePolicy::WriteBackWriteAllocate,
            WritePolicyArg::WbNwa => WritePolicy::WriteBackNoWriteAllocate,
            WritePolicyArg::WtWa => WritePolicy::WriteThroughWriteAllocate,
            WritePolicyArg::WtNwa => WritePolicy::WriteThroughNoWriteAllocate,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PrefetcherArg {
    /// No prefetching (default).
    Null,
    /// Stride-1 next-line prefetcher: fetch `addr + block_size` after
    /// every demand access.
    #[clap(name = "next-line")]
    #[serde(rename = "next-line")]
    NextLine,
}

impl From<PrefetcherArg> for crate::hardware::mem::general_cache::prefetcher::PrefetcherKind {
    fn from(a: PrefetcherArg) -> Self {
        use crate::hardware::mem::general_cache::prefetcher::PrefetcherKind;
        match a {
            PrefetcherArg::Null => PrefetcherKind::Null,
            PrefetcherArg::NextLine => PrefetcherKind::NextLine,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BranchPredictorArg {
    /// Always predicts not-taken.
    Dummy,
    /// Branch target table plus the historical bimodal direction counter.
    Bimodal,
}

impl From<BranchPredictorArg> for crate::sim::runner::BranchPredictorKind {
    fn from(a: BranchPredictorArg) -> Self {
        use crate::sim::runner::BranchPredictorKind;
        match a {
            BranchPredictorArg::Dummy => BranchPredictorKind::Dummy,
            BranchPredictorArg::Bimodal => BranchPredictorKind::Bimodal,
        }
    }
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackingMemoryArg {
    /// Flat-latency main memory.
    SimpleMem,
    /// Single-bank row-buffer-aware DRAM configured by the `--dram-*` flags.
    Dram,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::runner::BackingMemoryKind;

    #[test]
    fn dram_timing_flags_reach_runner_config() {
        let args = Args::try_parse_from([
            "simulator",
            "--prog",
            "qsort",
            "--memory",
            "dram",
            "--dram-trcd",
            "12",
            "--dram-tcl",
            "7",
            "--dram-trp",
            "9",
        ])
        .expect("parse CLI arguments");

        match args.backing_memory_kind() {
            BackingMemoryKind::SimpleDram { timing } => {
                assert_eq!(timing.t_rcd, 12);
                assert_eq!(timing.t_cl, 7);
                assert_eq!(timing.t_rp, 9);
            }
            BackingMemoryKind::SimpleMem => panic!("expected DRAM backing memory"),
        }
    }

    #[test]
    fn dram_timing_defaults_match_educational_model() {
        let args = Args::try_parse_from(["simulator", "--prog", "qsort", "--memory", "dram"])
            .expect("parse CLI arguments");

        match args.backing_memory_kind() {
            BackingMemoryKind::SimpleDram { timing } => {
                assert_eq!(timing.t_rcd, 4);
                assert_eq!(timing.t_cl, 4);
                assert_eq!(timing.t_rp, 4);
            }
            BackingMemoryKind::SimpleMem => panic!("expected DRAM backing memory"),
        }
    }

    #[test]
    fn zero_dram_timing_is_rejected_by_cli() {
        let parsed = Args::try_parse_from([
            "simulator",
            "--prog",
            "qsort",
            "--memory",
            "dram",
            "--dram-trcd",
            "0",
        ]);

        assert!(parsed.is_err());
    }
}
