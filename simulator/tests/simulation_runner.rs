use simulator::hardware::mem::simple_dram::DramTiming;
use simulator::sim::elf::ProgramInfo;
use simulator::sim::runner::{
    run, BackingMemoryKind, BackingMemoryReport, BranchPredictorKind, SimulationConfig,
};

const MEM_SIZE: usize = 0x40000;
const ECALL: u32 = 0x00000073;
const NOP: u32 = 0x00000013;

fn halt_program() -> ProgramInfo {
    let mut prog_body = vec![0u8; MEM_SIZE];
    for inst in prog_body.chunks_exact_mut(4) {
        inst.copy_from_slice(&NOP.to_le_bytes());
    }
    prog_body[0..4].copy_from_slice(&ECALL.to_le_bytes());
    ProgramInfo {
        entry_pc: 0,
        prog_body,
    }
}

#[test]
fn runner_halts_raw_program_and_reports_hierarchy_stats() {
    let report = run(halt_program(), SimulationConfig::default()).expect("run simulation");

    assert!(report.pipeline.inst_retire >= 1);
    assert!(report.pipeline.total_ticked_cycle > 0);
    assert_eq!(report.l1i.name, "L1-I$");
    assert_eq!(report.l1d.name, "L1-D$");
    assert_eq!(report.l2.name, "L2$");
    assert!(matches!(
        report.backing_memory,
        BackingMemoryReport::SimpleMem
    ));
}

#[test]
fn runner_composes_dram_and_predictor_adapters() {
    let config = SimulationConfig {
        backing_memory: BackingMemoryKind::SimpleDram {
            timing: DramTiming::educational_default(),
        },
        branch_predictor: BranchPredictorKind::Dummy,
        ..SimulationConfig::default()
    };

    let report = run(halt_program(), config).expect("run simulation");

    match report.backing_memory {
        BackingMemoryReport::SimpleDram {
            dram_access_cnt,
            cold_open_cnt,
            row_buffer_hit_cnt,
            row_buffer_miss_cnt,
            row_conflict_cnt,
            total_access_time_cycles,
            average_access_time_cycles,
        } => {
            assert_eq!(row_buffer_hit_cnt, 0);
            assert!(row_buffer_miss_cnt > 0);
            assert_eq!(dram_access_cnt, row_buffer_hit_cnt + row_buffer_miss_cnt);
            assert_eq!(row_buffer_miss_cnt, cold_open_cnt + row_conflict_cnt);
            assert!(total_access_time_cycles > 0);
            assert!(average_access_time_cycles > 0.0);
        }
        BackingMemoryReport::SimpleMem => panic!("expected DRAM report"),
    }
}

#[test]
fn runner_rejects_invalid_cache_geometry_before_controller_construction() {
    let config = SimulationConfig {
        l1d: simulator::sim::runner::CacheLevelConfig::new(256, 24, 2, 2),
        ..SimulationConfig::default()
    };

    let err = match run(halt_program(), config) {
        Ok(_) => panic!("invalid cache config should not run"),
        Err(err) => err,
    };

    assert_eq!(
        err.to_string(),
        "invalid L1-D$ cache config: block size must be a power of two, got 24"
    );
}
