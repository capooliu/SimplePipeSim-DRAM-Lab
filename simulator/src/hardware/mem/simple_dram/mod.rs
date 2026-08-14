//! Cycle-accurate single-bank DRAM controller.
//!
//! `SimpleDram` is a drop-in replacement for [`super::simple_mem::SimpleMem`]:
//! it implements [`AbstractMemoryInterface`] and [`Clocked`] so a
//! [`super::general_cache::GeneralCache`] can sit on top of it. Unlike
//! `SimpleMem`'s flat 15-cycle latency, this controller exposes
//! row-buffer hits, row-buffer misses, and bank-busy stalls — the
//! three knobs students need to observe in order to internalise why
//! DRAM is *not* a flat RAM.
//!
//! ## Timing model
//!
//! Per request, the controller picks one of three latency paths:
//!
//! | Path                       | Cycles to `done` | When it triggers                                |
//! |----------------------------|------------------|-------------------------------------------------|
//! | Cold open (`Idle`)         | `tRCD + tCL`     | First access since boot, or right after PRE.   |
//! | Row buffer **hit**         | `tCL`            | Bank is already `Active(req_row)`.             |
//! | Row buffer **miss**        | `tRP + tRCD + tCL` | Bank is `Active(other_row)`; must PRE then ACT. |
//!
//! Counts of hits / misses live in `row_buffer_hit_cnt` and
//! `row_buffer_miss_cnt`. Cold opens are counted as misses — they
//! issue an ACT just like any other miss path.
//!
//! ## Tick ordering
//!
//! `tick()` runs in **decide-then-advance** order each cycle:
//!
//! 1. Look at the bank's *current* state and the pending request; if
//!    something can be issued (ACT / CAS / PRE), issue it.
//! 2. Advance the bank by one cycle. If a CAS just finished, the
//!    bank emits a [`bank::BankEvent::ReadFinished`] /
//!    [`bank::BankEvent::WriteFinished`] and we complete the request
//!    in the same tick.
//!
//! This ordering is what makes the per-path latency numbers above
//! land on the nominal `tRCD + tCL` etc. — no extra off-by-one cycle
//! between command-issue and first countdown decrement.

pub mod bank;
pub mod timing;

pub use timing::DramTiming;

use crate::hardware::clock::Clocked;
use crate::hardware::mem::abstract_mem::*;
use bank::{Bank, BankEvent, BankState};

/// Default number of bytes that share one row buffer. Picked to be
/// power-of-two and large enough that typical cache blocks (32 / 64 B)
/// fit inside a single row.
pub const DEFAULT_ROW_SIZE_BYTES: usize = 1024;

/// Single-bank DRAM controller. Multi-bank support is intentionally
/// out of scope for this substrate PR; the controller wraps exactly
/// one [`Bank`] and the address decoder treats the whole address
/// space as row-major within that bank.
pub struct SimpleDram {
    /// Backing byte storage. Sized at construction; bounds-checked on
    /// every register call.
    data: Box<[u8]>,
    /// Per-bank latency budget (cycles).
    timing: DramTiming,
    /// Bytes per row buffer; configurable via
    /// [`SimpleDram::with_row_size`]. Must be a power of two so
    /// `addr / row_size` is cheap and the high address bits cleanly
    /// index the row.
    row_size_bytes: usize,
    /// The single bank.
    bank: Bank,
    /// Currently in-flight request, or `None` if the controller is
    /// idle. The request stays here until `done` is set.
    pending_req: Option<MemoryReqType>,
    /// Per-request flag: set to `true` on the cycle we issue an ACT
    /// for this request. Used to suppress a spurious hit-count bump
    /// when the request's own CAS later sees `Active(req_row)`.
    pending_req_required_activate: bool,
    /// Per-request flag: set when the request first finds a different
    /// row open and therefore has to issue PRE before ACT.
    pending_req_required_precharge: bool,

    /// Number of requests served entirely from an already-open row.
    /// Bumped at CAS-issue, on demand requests only.
    pub row_buffer_hit_cnt: usize,
    /// Number of requests that required an ACT (cold open or
    /// post-precharge). Bumped at ACT-issue.
    pub row_buffer_miss_cnt: usize,
    /// Number of requests that opened an idle bank without first
    /// precharging a different row.
    pub cold_open_cnt: usize,
    /// Number of requests that had to precharge a different open row.
    pub row_conflict_cnt: usize,
    /// Sum of the modeled DRAM service latency of every request.
    pub total_access_time_cycles: usize,
}

impl SimpleDram {
    /// Build a controller backed by `init_data` bytes and the given
    /// timing parameters. Defaults to [`DEFAULT_ROW_SIZE_BYTES`]; use
    /// [`SimpleDram::with_row_size`] to override.
    pub fn new(init_data: Vec<u8>, timing: DramTiming) -> Self {
        timing.assert_valid();
        Self {
            data: init_data.into_boxed_slice(),
            timing,
            row_size_bytes: DEFAULT_ROW_SIZE_BYTES,
            bank: Bank::new(),
            pending_req: None,
            pending_req_required_activate: false,
            pending_req_required_precharge: false,
            row_buffer_hit_cnt: 0,
            row_buffer_miss_cnt: 0,
            cold_open_cnt: 0,
            row_conflict_cnt: 0,
            total_access_time_cycles: 0,
        }
    }

    /// Override the row size. `row_size_bytes` must be a power of two
    /// and >= 1.
    pub fn with_row_size(mut self, row_size_bytes: usize) -> Self {
        assert!(
            row_size_bytes >= 1 && row_size_bytes.is_power_of_two(),
            "row_size_bytes must be a power of two and >= 1, got {}",
            row_size_bytes
        );
        self.row_size_bytes = row_size_bytes;
        self
    }

    /// Read-only access to the controller's timing constants — handy
    /// for tests asserting against the latency formulas.
    pub fn timing(&self) -> DramTiming {
        self.timing
    }

    /// Total number of completed or in-flight DRAM requests that have
    /// already been classified as a hit, cold open, or row conflict.
    pub fn dram_access_cnt(&self) -> usize {
        self.row_buffer_hit_cnt + self.row_buffer_miss_cnt
    }

    /// Mean modeled DRAM service latency per classified request.
    pub fn average_access_time_cycles(&self) -> f64 {
        let access_cnt = self.dram_access_cnt();
        if access_cnt == 0 {
            0.0
        } else {
            self.total_access_time_cycles as f64 / access_cnt as f64
        }
    }

    fn addr_to_row(&self, addr: u32) -> u32 {
        addr / (self.row_size_bytes as u32)
    }

    fn complete_pending_read(&mut self) {
        let req = self
            .pending_req
            .take()
            .expect("complete_pending_read called with no pending request");
        let addr = req.get_addr() as usize;
        let len = req.get_len();
        req.complete_load_from_slice(&self.data[addr..(addr + len)]);
        self.pending_req_required_activate = false;
        self.pending_req_required_precharge = false;
    }

    fn complete_pending_write(&mut self) {
        let req = self
            .pending_req
            .take()
            .expect("complete_pending_write called with no pending request");
        let addr = req.get_addr() as usize;
        let len = req.get_len();
        self.data[addr..(addr + len)].clone_from_slice(req.store_data());
        req.complete_store();
        self.pending_req_required_activate = false;
        self.pending_req_required_precharge = false;
    }
}

impl AbstractMemoryInterface for SimpleDram {
    fn try_register_req(&mut self, req: &MemoryReqType) -> Result<(), ()> {
        let addr = req.get_addr() as usize;
        let len = req.get_len();
        assert!(
            addr + len <= self.data.len(),
            "Out-of-bound DRAM access: addr={:#010X} len={} (data.len={})",
            addr,
            len,
            self.data.len()
        );
        // SimpleDram is a single-bank controller; one outstanding
        // request at a time. The caller is expected to retry on Err.
        if self.pending_req.is_some() {
            return Err(());
        }
        self.pending_req = Some(req.clone());
        self.pending_req_required_activate = false;
        self.pending_req_required_precharge = false;
        Ok(())
    }
}

impl Clocked for SimpleDram {
    /// One tick = one core clock cycle. See the module docs for the
    /// decide-then-advance contract.
    fn tick(&mut self) {
        // -------- Phase 1: decide what command (if any) to issue --------
        //
        // We borrow `pending_req` only as a `&` here so we can mutate
        // the bank and counters without fighting the borrow checker;
        // anything that actually consumes the request happens in
        // Phase 2's completion path.
        if let Some(req) = self.pending_req.as_ref() {
            let req_row = self.addr_to_row(req.get_addr());
            let req_is_store = matches!(req, MemoryReqType::Store(_));
            match self.bank.state() {
                BankState::Idle => {
                    // Cold open or post-precharge. Either way this
                    // request paid the cost of an ACT, so it counts
                    // as a row-buffer miss.
                    self.row_buffer_miss_cnt += 1;
                    if !self.pending_req_required_precharge {
                        self.cold_open_cnt += 1;
                        self.total_access_time_cycles += self.timing.t_rcd + self.timing.t_cl;
                    }
                    self.pending_req_required_activate = true;
                    self.bank.issue_activate(req_row, self.timing.t_rcd);
                }
                BankState::Active(open_row) if open_row == req_row => {
                    // The row we want is already open. If we got here
                    // without ever issuing ACT for this request, it's
                    // a genuine row-buffer hit. Otherwise it's the
                    // tail end of a miss request and the miss was
                    // already counted at ACT-issue.
                    if !self.pending_req_required_activate {
                        self.row_buffer_hit_cnt += 1;
                        self.total_access_time_cycles += self.timing.t_cl;
                    }
                    if req_is_store {
                        self.bank.issue_write(req_row, self.timing.t_cl);
                    } else {
                        self.bank.issue_read(req_row, self.timing.t_cl);
                    }
                }
                BankState::Active(_) => {
                    // Wrong row open: precharge, then the next pass
                    // through this `match` will see Idle and ACT the
                    // request's row.
                    self.row_conflict_cnt += 1;
                    self.total_access_time_cycles +=
                        self.timing.t_rp + self.timing.t_rcd + self.timing.t_cl;
                    self.pending_req_required_precharge = true;
                    self.bank.issue_precharge(self.timing.t_rp);
                }
                BankState::Activating { .. }
                | BankState::Precharging { .. }
                | BankState::Reading { .. }
                | BankState::Writing { .. } => {
                    // Bank is mid-command; no new command can be
                    // issued this cycle. Phase 2 will advance the
                    // countdown.
                }
            }
        }

        // -------- Phase 2: advance the bank one cycle --------
        //
        // If the cycle we just advanced through was the trigger cycle
        // of a CAS, the bank emits the appropriate event and we
        // complete the request right here.
        match self.bank.advance_one_cycle() {
            Some(BankEvent::ReadFinished) => self.complete_pending_read(),
            Some(BankEvent::WriteFinished) => self.complete_pending_write(),
            Some(BankEvent::ActivationFinished) | Some(BankEvent::PrechargeFinished) | None => {
                // Activation / precharge events are purely
                // informational; the next tick's Phase 1 will pick
                // up the new bank state and issue the follow-up
                // command.
            }
        }
    }
}

#[cfg(test)]
mod unit_tests {
    use super::*;
    fn build_dram(row_size: usize) -> SimpleDram {
        SimpleDram::new(vec![0u8; 0x10000], DramTiming::educational_default())
            .with_row_size(row_size)
    }

    /// Register `req`, then tick the DRAM until `done` is set; return
    /// the number of ticks observed. Panics if the request never
    /// completes within a generous safety budget.
    fn run_to_completion(dram: &mut SimpleDram, req: &MemoryReqType) -> usize {
        dram.try_register_req(req).expect("register request");
        let mut cycles = 0usize;
        while !req.is_done() {
            dram.tick();
            cycles += 1;
            assert!(
                cycles < 10_000,
                "request never completed (state stuck somewhere?)"
            );
        }
        cycles
    }

    fn make_load(addr: u32, len: usize) -> MemoryReqType {
        MemoryReqType::load(addr, len)
    }

    fn make_store(addr: u32, data: Vec<u8>) -> MemoryReqType {
        MemoryReqType::store(addr, data)
    }

    #[test]
    fn cold_load_takes_t_rcd_plus_t_cl_cycles() {
        let mut d = build_dram(256);
        let t = d.timing();
        let req = make_load(0x100, 4); // row = 0x100 / 256 = 1
        let cycles = run_to_completion(&mut d, &req);
        assert_eq!(cycles, t.t_rcd + t.t_cl);
        assert_eq!(d.row_buffer_miss_cnt, 1);
        assert_eq!(d.row_buffer_hit_cnt, 0);
        assert_eq!(d.cold_open_cnt, 1);
        assert_eq!(d.row_conflict_cnt, 0);
        assert_eq!(d.total_access_time_cycles, cycles);
        assert_eq!(d.dram_access_cnt(), 1);
        assert_eq!(d.average_access_time_cycles(), cycles as f64);
    }

    #[test]
    fn row_buffer_hit_takes_only_t_cl_cycles() {
        let mut d = build_dram(256);
        let t = d.timing();
        // Prime: cold-load opens row 1.
        let warm = make_load(0x100, 4);
        run_to_completion(&mut d, &warm);
        // Second access into the same row.
        let hit = make_load(0x110, 4);
        let cycles = run_to_completion(&mut d, &hit);
        assert_eq!(
            cycles, t.t_cl,
            "row buffer hit must take exactly tCL cycles"
        );
        assert_eq!(d.row_buffer_hit_cnt, 1);
        assert_eq!(d.row_buffer_miss_cnt, 1);
        assert_eq!(d.cold_open_cnt, 1);
        assert_eq!(d.row_conflict_cnt, 0);
        assert_eq!(d.total_access_time_cycles, t.t_rcd + 2 * t.t_cl);
    }

    #[test]
    fn row_buffer_miss_takes_t_rp_plus_t_rcd_plus_t_cl() {
        let mut d = build_dram(256);
        let t = d.timing();
        // Open row 1, then access row 2 — full miss path.
        let prime = make_load(0x100, 4);
        run_to_completion(&mut d, &prime);
        let miss = make_load(0x200, 4); // row 2
        let cycles = run_to_completion(&mut d, &miss);
        assert_eq!(cycles, t.t_rp + t.t_rcd + t.t_cl);
        assert_eq!(d.row_buffer_miss_cnt, 2);
        assert_eq!(d.row_buffer_hit_cnt, 0);
        assert_eq!(d.cold_open_cnt, 1);
        assert_eq!(d.row_conflict_cnt, 1);
        assert_eq!(
            d.total_access_time_cycles,
            (t.t_rcd + t.t_cl) + (t.t_rp + t.t_rcd + t.t_cl)
        );
    }

    #[test]
    fn store_then_load_round_trips_data() {
        let mut d = build_dram(256);
        let s = make_store(0x40, vec![0xDE, 0xAD, 0xBE, 0xEF]);
        run_to_completion(&mut d, &s);
        let l = make_load(0x40, 4);
        run_to_completion(&mut d, &l);
        let buf = l.load_data();
        assert_eq!(&buf[..], &[0xDE, 0xAD, 0xBE, 0xEF]);
    }

    #[test]
    fn store_to_open_row_is_a_hit() {
        let mut d = build_dram(256);
        let t = d.timing();
        // Cold-load opens row 0.
        let prime = make_load(0x00, 4);
        run_to_completion(&mut d, &prime);
        // Store to the same row.
        let s = make_store(0x10, vec![1, 2, 3, 4]);
        let cycles = run_to_completion(&mut d, &s);
        assert_eq!(cycles, t.t_cl, "store hit must take tCL cycles");
        assert_eq!(d.row_buffer_hit_cnt, 1);
    }

    #[test]
    fn second_request_rejected_while_one_is_in_flight() {
        let mut d = build_dram(256);
        let first = make_load(0x40, 4);
        d.try_register_req(&first).expect("first register");
        // Without ticking, the bank is still mid-activation when we
        // try the second register.
        let second = make_load(0x80, 4);
        assert!(
            d.try_register_req(&second).is_err(),
            "DRAM must reject second outstanding request"
        );
    }

    #[test]
    #[should_panic(expected = "Out-of-bound DRAM access")]
    fn out_of_bound_access_panics() {
        let mut d = build_dram(256);
        let req = make_load(0xFFFF_0000, 4);
        let _ = d.try_register_req(&req);
    }
}
