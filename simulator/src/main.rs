use clap::Parser;

use simulator::sim::cli::Args;
use simulator::sim::elf;
use simulator::sim::runner::{self, CacheLevelConfig, SimulationConfig, SimulationReport};

use std::error::Error;

#[derive(serde::Serialize)]
struct CliRunReport<'a> {
    #[serde(flatten)]
    simulation: &'a SimulationReport,
    config: &'a Args,
}

fn simulation_config(args: &Args) -> SimulationConfig {
    SimulationConfig {
        l1i: CacheLevelConfig::new(
            args.l1i_size,
            args.l1i_block,
            args.l1i_ways,
            args.l1i_penalty,
        ),
        l1d: CacheLevelConfig::new(
            args.l1d_size,
            args.l1d_block,
            args.l1d_ways,
            args.l1d_penalty,
        ),
        l2: CacheLevelConfig::new(args.l2_size, args.l2_block, args.l2_ways, args.l2_penalty),
        replacement_policy: args.rp.into(),
        write_policy: args.wp.into(),
        prefetcher_kind: args.prefetcher.into(),
        backing_memory: args.backing_memory_kind(),
        branch_predictor: args.bp.into(),
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    let elf_path = args.elf_dir.join(&args.prog);
    let elf::ProgramInfo {
        entry_pc,
        prog_body,
    } = elf::elf_loader(&elf_path);

    let simulation = runner::run(
        elf::ProgramInfo {
            entry_pc,
            prog_body,
        },
        simulation_config(&args),
    )?;

    println!("Program: {}", args.prog);
    println!("Replacement policy: {:?}", args.rp);
    println!("Branch predictor: {:?}", args.bp);
    println!("Backing memory: {:?}", args.memory);
    println!(
        "Pipeline: cycles={} retired={} ipc={:.4} branch_miss={:.4}",
        simulation.pipeline.total_ticked_cycle,
        simulation.pipeline.inst_retire,
        simulation.pipeline.ipc,
        simulation.pipeline.branch_miss_rate
    );
    for (label, s) in [
        ("L1-I$", &simulation.l1i),
        ("L1-D$", &simulation.l1d),
        ("L2$  ", &simulation.l2),
    ] {
        println!(
            "  {} load_cnt={} load_miss={} ({:.4})  store_cnt={} store_miss={} ({:.4})  overall_miss={:.4}",
            label,
            s.load_cnt,
            s.load_miss_cnt,
            s.load_miss_rate,
            s.store_cnt,
            s.store_miss_cnt,
            s.store_miss_rate,
            s.overall_miss_rate
        );
    }

    if let Some(path) = &args.stats_out {
        let report = CliRunReport {
            simulation: &simulation,
            config: &args,
        };
        let json = serde_json::to_string_pretty(&report)?;
        std::fs::write(path, json)?;
        println!("wrote stats JSON to {}", path.display());
    }

    Ok(())
}
