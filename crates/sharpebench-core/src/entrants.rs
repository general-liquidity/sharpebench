//! Reference entrants: specified, deterministic signal primitives.
//!
//! Every entrant the paper has reported so far was written by the author, which
//! leaves a reader unable to tell a strict gate from a weak field. The harness
//! already scores four rules taken from the published literature (Donchian
//! channel breakout, the Brock-Lakonishok-LeBaron variable moving average,
//! Faber's ten-month filter, Wilder's RSI). This module adds seven more in the
//! same spirit, drawn from patterns that are widely published and widely traded.
//!
//! These are **entrants, not infrastructure**. Nothing in the scoring kernel
//! calls them. They exist to be scored *by* it, through exactly the gates every
//! other entrant passes through, so the field has specified opponents whose
//! behavior a reader can check line by line.
//!
//! House rules, which are what make them scoreable:
//!
//! - **State in, state out.** Every stateful primitive takes its previous state
//!   by value and returns the next one. There is no hidden state, no interior
//!   mutability, and no clock: replaying the same inputs from the same starting
//!   state reproduces the same outputs on any platform.
//! - **Caller-supplied thresholds.** No primitive fits a parameter to the data
//!   it is fed. Defaults are published starting points, named as such, not
//!   values discovered here.
//! - **Pre-computed numeric inputs.** RSI, ADX, ATR and moving averages arrive
//!   as numbers, so an entrant can be wired onto any bar source without this
//!   module owning an indicator library.
//! - **No I/O and no randomness.**
//!
//! Each primitive's docstring names its source and its parameters.

use serde::{Deserialize, Serialize};

/// Which way a primitive wants to be positioned.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[default]
    None,
    Long,
    Short,
}

impl Side {
    /// The side that closes this one.
    pub fn opposite(self) -> Side {
        match self {
            Side::Long => Side::Short,
            Side::Short => Side::Long,
            Side::None => Side::None,
        }
    }
}

// ---------------------------------------------------------------------------
// 1. Regime-modulated RSI.
// ---------------------------------------------------------------------------

/// The trend context a regime-aware rule switches on.
///
/// Deliberately coarse: three buckets, because the point of the rule is that a
/// reading means different things in a trend than in a range, and a finer
/// taxonomy would need fitting.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TrendRegime {
    /// A confirmed uptrend, or a breakout from one.
    Bull,
    /// A confirmed downtrend, or a volatile break lower.
    Bear,
    /// Ranging or quiet: no directional context.
    Idle,
}

/// The overbought/oversold band pair for one regime, with its ADX modulation.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct RsiBands {
    /// RSI level above which the rule goes short.
    pub rsi_high: f64,
    /// RSI level below which the rule goes long.
    pub rsi_low: f64,
    /// Added to `rsi_high` when ADX is above `RegimeRsiParams::adx_high`.
    pub mod_high: f64,
    /// Added to `rsi_low` when ADX is below `RegimeRsiParams::adx_low`. Negative
    /// in the published defaults: a weak trend pushes the oversold line down.
    pub mod_low: f64,
}

/// Parameters of the regime-modulated RSI rule.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct RegimeRsiParams {
    pub bull: RsiBands,
    pub bear: RsiBands,
    pub idle: RsiBands,
    /// ADX above this counts as a strong trend.
    pub adx_high: f64,
    /// ADX below this counts as a weak trend.
    pub adx_low: f64,
}

impl Default for RegimeRsiParams {
    /// The conventional starting point, not a fitted set.
    ///
    /// Bull: slow to short into strength (75), eager to buy dips (35). Bear:
    /// eager to short rallies (60), slow to catch knives (25). Idle: the
    /// textbook symmetric 70/30. ADX 30/15 are Wilder's usual strong/weak marks.
    fn default() -> Self {
        Self {
            bull: RsiBands {
                rsi_high: 75.0,
                rsi_low: 35.0,
                mod_high: 5.0,
                mod_low: -5.0,
            },
            bear: RsiBands {
                rsi_high: 60.0,
                rsi_low: 25.0,
                mod_high: 5.0,
                mod_low: -5.0,
            },
            idle: RsiBands {
                rsi_high: 70.0,
                rsi_low: 30.0,
                mod_high: 3.0,
                mod_low: -3.0,
            },
            adx_high: 30.0,
            adx_low: 15.0,
        }
    }
}

/// What the regime-modulated RSI rule decided, and the bands it decided against.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct RegimeRsiSignal {
    pub side: Side,
    pub regime: TrendRegime,
    /// The overbought line after ADX modulation.
    pub effective_high: f64,
    /// The oversold line after ADX modulation.
    pub effective_low: f64,
}

/// Regime-modulated RSI: the same RSI reading, read differently by trend context.
///
/// **Source.** Wilder's RSI (J. Welles Wilder Jr., *New Concepts in Technical
/// Trading Systems*, 1978) with its 70/30 bands, combined with the standard
/// practitioner adjustment that oscillator bands should shift with trend
/// strength, measured by Wilder's ADX from the same book. In a confirmed
/// uptrend a low RSI is a dip to buy; in a downtrend the same reading is a
/// falling knife, and the rally to fade is the high one.
///
/// **Parameters.** `params` carries three band pairs (bull, bear, idle) and the
/// two ADX cutoffs. A strong trend (`adx > adx_high`) raises the overbought line
/// by `mod_high`, so the rule does not fade strength. A weak trend
/// (`adx < adx_low`) shifts the oversold line by `mod_low`, so it does not buy a
/// slow bleed. Only one modulation can apply, strong taking precedence.
///
/// **Inputs.** `rsi` and `adx` are pre-computed at the current bar. Stateless.
pub fn regime_rsi(
    regime: TrendRegime,
    rsi: f64,
    adx: f64,
    params: &RegimeRsiParams,
) -> RegimeRsiSignal {
    let bands = match regime {
        TrendRegime::Bull => params.bull,
        TrendRegime::Bear => params.bear,
        TrendRegime::Idle => params.idle,
    };
    let mut effective_high = bands.rsi_high;
    let mut effective_low = bands.rsi_low;
    if adx > params.adx_high {
        effective_high += bands.mod_high;
    } else if adx < params.adx_low {
        effective_low += bands.mod_low;
    }

    let side = if rsi > effective_high {
        Side::Short
    } else if rsi < effective_low {
        Side::Long
    } else {
        Side::None
    };
    RegimeRsiSignal {
        side,
        regime,
        effective_high,
        effective_low,
    }
}

// ---------------------------------------------------------------------------
// 2. Bounce counter.
// ---------------------------------------------------------------------------

/// Which extreme zone an oscillator currently sits in.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    High,
    Low,
    #[default]
    Neutral,
}

/// State of the bounce counter. Owned by the caller.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct BounceState {
    pub zone: Zone,
    /// Bars spent in the current zone, counting the bar it was entered on.
    pub duration: u32,
    /// Re-entries into an extreme zone since the last reset.
    pub bounces: u32,
    /// True once the zone has been left, arming the next entry to count.
    pub armed: bool,
    /// Consecutive neutral bars.
    pub flats: u32,
    /// True once this occupancy of the zone has fired, so it fires once.
    pub fired: bool,
}

/// Parameters of the bounce counter.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct BounceParams {
    /// Oscillator level above which the zone is `High`.
    pub high: f64,
    /// Oscillator level below which the zone is `Low`.
    pub low: f64,
    /// Bars the zone must be held before it can fire.
    pub persistence: u32,
    /// Re-entries required before a persisted zone fires.
    pub required_bounces: u32,
    /// Consecutive neutral bars that reset the bounce count.
    pub reset_after_flats: u32,
}

impl Default for BounceParams {
    /// The textbook 70/30 oscillator bands, three bars of persistence, two
    /// bounces, and a fifty-bar amnesia. Starting points, not fitted values.
    fn default() -> Self {
        Self {
            high: 70.0,
            low: 30.0,
            persistence: 3,
            required_bounces: 2,
            reset_after_flats: 50,
        }
    }
}

/// Bounce counter: require a repeated visit to an extreme before acting on it.
///
/// **Source.** The practitioner observation, standard in divergence-based
/// oscillator trading, that a single touch of an overbought or oversold level is
/// noise: a slow grind revisits the same level repeatedly and only a later visit
/// marks the turn. The rule counts *re-entries* into the extreme zone after the
/// oscillator has left it, and fires only when the zone has both persisted and
/// been re-entered often enough. It is a confirmation filter, not a signal
/// generator, so it always fades the extreme: a persisted `High` fires short.
///
/// **Parameters.** `high` and `low` are the zone boundaries, `persistence` the
/// bars the zone must be held, `required_bounces` the re-entries demanded, and
/// `reset_after_flats` the run of neutral bars that clears the count.
///
/// **State.** Takes the previous [`BounceState`] and returns the next one along
/// with the side to trade. Fires at most once per occupancy of a zone.
pub fn bounce_counter(prev: BounceState, oscillator: f64, p: &BounceParams) -> (Side, BounceState) {
    let zone = if oscillator > p.high {
        Zone::High
    } else if oscillator < p.low {
        Zone::Low
    } else {
        Zone::Neutral
    };

    let mut next = prev;
    if zone != prev.zone {
        if zone == Zone::Neutral {
            next.armed = true;
        } else if prev.armed {
            next.bounces = prev.bounces + 1;
            next.armed = false;
        }
        next.zone = zone;
        next.duration = 1;
        next.fired = false;
        next.flats = if zone == Zone::Neutral {
            prev.flats + 1
        } else {
            0
        };
    } else {
        next.duration = prev.duration + 1;
        if zone == Zone::Neutral {
            next.flats = prev.flats + 1;
            if next.flats >= p.reset_after_flats {
                next.bounces = 0;
                next.flats = 0;
                next.armed = false;
            }
        } else {
            next.flats = 0;
        }
    }

    let mut side = Side::None;
    if !next.fired
        && next.zone != Zone::Neutral
        && next.duration >= p.persistence
        && next.bounces >= p.required_bounces
    {
        side = if next.zone == Zone::High {
            Side::Short
        } else {
            Side::Long
        };
        next.fired = true;
    }
    (side, next)
}

// ---------------------------------------------------------------------------
// 3. Signal gate (trend confirmation before execution).
// ---------------------------------------------------------------------------

/// What the signal gate did with the raw signal it was handed.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    /// The moving averages already agreed, so the signal went straight through.
    ExecutedImmediately,
    /// A parked signal was released once the moving averages agreed.
    ExecutedAfterConfirmation,
    /// No signal to act on.
    Passthrough,
    /// The signal is parked, waiting for confirmation.
    Pending,
    /// A parked signal was dropped because a fresh signal pointed the other way.
    Cancelled,
}

/// State of the signal gate. Owned by the caller.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct GateState {
    pub pending: Side,
    /// Price when the parked signal was first raised.
    pub pending_price: f64,
    /// Bars the current signal has been parked.
    pub waited: u32,
}

/// What the signal gate decided this bar.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct GateDecision {
    pub execute: Side,
    pub status: GateStatus,
    /// Fill improvement from waiting, as a fraction of the signal price and
    /// signed so positive is better. Present only on
    /// [`GateStatus::ExecutedAfterConfirmation`].
    pub benefit: Option<f64>,
}

/// Signal gate: hold an entry until a trend-following reference confirms it.
///
/// **Source.** The moving-average crossover as a timing reference, the oldest
/// published trend filter there is, used here in the role Brock, Lakonishok and
/// LeBaron (1992) tested it in: not as the signal, but as the confirmation that
/// the trend has actually turned. An oscillator can be right about direction and
/// early about timing, so the gate separates *what* to trade from *when*.
///
/// **Parameters.** `fast_ma` and `slow_ma` are pre-computed at the current bar;
/// a long is confirmed when `fast_ma >= slow_ma` and a short when
/// `fast_ma <= slow_ma`. `max_wait` bounds how long a signal may sit parked; a
/// signal that waits longer is cancelled rather than filled on stale conviction.
///
/// **State.** Takes the previous [`GateState`] and returns the next one. A
/// parked signal is cancelled early if a fresh signal points the other way.
pub fn signal_gate(
    prev: GateState,
    raw: Side,
    fast_ma: f64,
    slow_ma: f64,
    price: f64,
    max_wait: u32,
) -> (GateDecision, GateState) {
    let confirms = |side: Side| match side {
        Side::Long => fast_ma >= slow_ma,
        Side::Short => fast_ma <= slow_ma,
        Side::None => false,
    };

    if prev.pending != Side::None {
        if confirms(prev.pending) {
            let benefit = if prev.pending_price == 0.0 {
                0.0
            } else if prev.pending == Side::Long {
                (prev.pending_price - price) / prev.pending_price
            } else {
                (price - prev.pending_price) / prev.pending_price
            };
            return (
                GateDecision {
                    execute: prev.pending,
                    status: GateStatus::ExecutedAfterConfirmation,
                    benefit: Some(benefit),
                },
                GateState::default(),
            );
        }
        if raw != Side::None && raw != prev.pending {
            return (
                GateDecision {
                    execute: Side::None,
                    status: GateStatus::Cancelled,
                    benefit: None,
                },
                GateState::default(),
            );
        }
        let waited = prev.waited + 1;
        if waited >= max_wait {
            return (
                GateDecision {
                    execute: Side::None,
                    status: GateStatus::Cancelled,
                    benefit: None,
                },
                GateState::default(),
            );
        }
        return (
            GateDecision {
                execute: Side::None,
                status: GateStatus::Pending,
                benefit: None,
            },
            GateState { waited, ..prev },
        );
    }

    if raw == Side::None {
        return (
            GateDecision {
                execute: Side::None,
                status: GateStatus::Passthrough,
                benefit: None,
            },
            prev,
        );
    }

    if confirms(raw) {
        return (
            GateDecision {
                execute: raw,
                status: GateStatus::ExecutedImmediately,
                benefit: None,
            },
            prev,
        );
    }

    (
        GateDecision {
            execute: Side::None,
            status: GateStatus::Pending,
            benefit: None,
        },
        GateState {
            pending: raw,
            pending_price: price,
            waited: 1,
        },
    )
}

// ---------------------------------------------------------------------------
// 4. Max-exposure timeout (the time stop).
// ---------------------------------------------------------------------------

/// State of the time stop. Owned by the caller.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct ExposureState {
    /// Side of the open position, or `None` when flat.
    pub side: Side,
    pub entry_price: f64,
    pub bars_held: u32,
}

/// What the time stop decided this bar.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct ExposureDecision {
    /// The trade to place now. `None` means hold.
    pub action: Side,
    /// True when the action was forced by the clock rather than by a signal, so
    /// an attribution can separate time stops from thesis changes.
    pub forced_exit: bool,
}

/// Time stop: close a position after a fixed number of bars regardless of P&L.
///
/// **Source.** The time stop, standard in mean-reversion practice and stated
/// explicitly as one of the three exit types in Perry Kaufman's *Trading Systems
/// and Methods*. A mean-reversion entry fails in two ways: the move continues
/// against you, which a price stop handles, or the price chops sideways and
/// never resolves, which a price stop never sees. The second failure is
/// invisible to a P&L-only exit and quietly ties up capital, so it is capped by
/// the clock instead.
///
/// **Parameters.** `max_bars` is the holding limit. It is the only parameter;
/// the rule takes its entries from whatever upstream signal is handed to it.
///
/// **State.** Takes the previous [`ExposureState`] and returns the next one. An
/// opposing signal closes the position normally and is not reported as forced.
pub fn max_exposure_timeout(
    prev: ExposureState,
    signal: Side,
    price: f64,
    max_bars: u32,
) -> (ExposureDecision, ExposureState) {
    if prev.side == Side::None {
        if signal == Side::None {
            return (
                ExposureDecision {
                    action: Side::None,
                    forced_exit: false,
                },
                prev,
            );
        }
        return (
            ExposureDecision {
                action: signal,
                forced_exit: false,
            },
            ExposureState {
                side: signal,
                entry_price: price,
                bars_held: 0,
            },
        );
    }

    let bars_held = prev.bars_held + 1;
    if bars_held >= max_bars {
        return (
            ExposureDecision {
                action: prev.side.opposite(),
                forced_exit: true,
            },
            ExposureState::default(),
        );
    }
    if signal != Side::None && signal != prev.side {
        return (
            ExposureDecision {
                action: signal,
                forced_exit: false,
            },
            ExposureState::default(),
        );
    }
    (
        ExposureDecision {
            action: Side::None,
            forced_exit: false,
        },
        ExposureState { bars_held, ..prev },
    )
}

// ---------------------------------------------------------------------------
// 5. ATR breakout with a volatility-expansion filter.
// ---------------------------------------------------------------------------

/// Parameters of the ATR breakout rule.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct AtrBreakoutParams {
    /// Entry-stop distance from the bar open, in units of the long ATR.
    pub entry_multiplier: f64,
    /// Exit-stop distance from the entry, in units of the long ATR. Tighter than
    /// the entry multiplier in the published defaults, so losers are cut faster
    /// than winners are let run.
    pub exit_multiplier: f64,
    /// The short ATR must exceed this multiple of the long ATR for the breakout
    /// to arm. Above 1.0 means volatility must be actively expanding.
    pub expansion_ratio: f64,
    /// Long ATR below this is treated as too quiet to trade.
    pub min_atr_long: f64,
}

impl Default for AtrBreakoutParams {
    /// The conventional 2.5x entry and 1.0x exit pair with a 1.0 expansion
    /// requirement. Published starting points for index futures on intraday
    /// bars, not values fitted here.
    fn default() -> Self {
        Self {
            entry_multiplier: 2.5,
            exit_multiplier: 1.0,
            expansion_ratio: 1.0,
            min_atr_long: 0.0,
        }
    }
}

/// Why the ATR breakout rule did or did not arm.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AtrBreakoutVerdict {
    /// Stops are live at the returned levels.
    Armed,
    /// Volatility is not expanding: the short ATR did not clear the ratio.
    FilteredNoExpansion,
    /// The long ATR is below `min_atr_long`.
    FilteredLowVol,
    /// A non-finite or non-positive input.
    InvalidInput,
}

/// The stop levels an armed ATR breakout places.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct AtrBreakoutLevels {
    pub long_entry_stop: f64,
    pub short_entry_stop: f64,
    /// Distance from the entry to its protective stop.
    pub exit_stop_distance: f64,
    pub long_stop_loss: f64,
    pub short_stop_loss: f64,
}

/// What the ATR breakout rule decided at this bar.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct AtrBreakoutSignal {
    pub verdict: AtrBreakoutVerdict,
    /// `short_atr / long_atr`. 0.0 when the inputs were rejected.
    pub expansion_ratio: f64,
    /// Present only when the verdict is [`AtrBreakoutVerdict::Armed`].
    pub levels: Option<AtrBreakoutLevels>,
}

/// ATR breakout: bracketing stops at a volatility-scaled distance, armed only
/// while volatility is expanding.
///
/// **Source.** Wilder's Average True Range (J. Welles Wilder Jr., *New Concepts
/// in Technical Trading Systems*, 1978) used in its volatility-breakout role:
/// place stop entries above and below the bar's open at a multiple of ATR, so
/// the trigger distance scales with the market's own noise instead of a fixed
/// point value. The expansion filter is the standard refinement that a breakout
/// worth taking happens while range is widening, measured as a short ATR above a
/// long one; a breakout into contracting range is the classic false break.
///
/// **Parameters.** `entry_multiplier` and `exit_multiplier` scale the entry and
/// protective-stop distances by the long ATR. `expansion_ratio` is the
/// short-over-long ATR the rule requires. `min_atr_long` is a floor below which
/// the market is treated as untradeably quiet.
///
/// **Inputs.** `bar_open`, `atr_short` and `atr_long` are pre-computed.
/// Stateless: the rule re-derives its levels from scratch each bar.
pub fn atr_breakout(
    bar_open: f64,
    atr_short: f64,
    atr_long: f64,
    p: &AtrBreakoutParams,
) -> AtrBreakoutSignal {
    if !bar_open.is_finite()
        || !atr_short.is_finite()
        || !atr_long.is_finite()
        || atr_long <= 0.0
        || atr_short < 0.0
    {
        return AtrBreakoutSignal {
            verdict: AtrBreakoutVerdict::InvalidInput,
            expansion_ratio: 0.0,
            levels: None,
        };
    }
    let ratio = atr_short / atr_long;
    if atr_long < p.min_atr_long {
        return AtrBreakoutSignal {
            verdict: AtrBreakoutVerdict::FilteredLowVol,
            expansion_ratio: ratio,
            levels: None,
        };
    }
    if ratio <= p.expansion_ratio {
        return AtrBreakoutSignal {
            verdict: AtrBreakoutVerdict::FilteredNoExpansion,
            expansion_ratio: ratio,
            levels: None,
        };
    }

    let entry_distance = atr_long * p.entry_multiplier;
    let exit_distance = atr_long * p.exit_multiplier;
    let long_entry_stop = bar_open + entry_distance;
    let short_entry_stop = bar_open - entry_distance;
    AtrBreakoutSignal {
        verdict: AtrBreakoutVerdict::Armed,
        expansion_ratio: ratio,
        levels: Some(AtrBreakoutLevels {
            long_entry_stop,
            short_entry_stop,
            exit_stop_distance: exit_distance,
            long_stop_loss: long_entry_stop - exit_distance,
            short_stop_loss: short_entry_stop + exit_distance,
        }),
    }
}

// ---------------------------------------------------------------------------
// 6 and 7. Distribution days and the follow-through day.
// ---------------------------------------------------------------------------

/// One bar of the index series the two O'Neil rules read.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct IndexBar {
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// Parameters of the distribution-day count.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct DistributionParams {
    /// Close-to-close fraction, negative, at or below which a session counts as
    /// a down day.
    pub down_threshold: f64,
    /// Trailing sessions over which live days are counted.
    pub window: usize,
    /// Rally from a distribution day's close that invalidates it.
    pub invalidation_gain: f64,
    /// Whether volume must exceed the prior session's.
    pub volume_must_rise: bool,
}

impl Default for DistributionParams {
    /// O'Neil's published figures: a down close of at least 0.2% on rising
    /// volume, counted over 25 sessions, cancelled by a 5% rally.
    fn default() -> Self {
        Self {
            down_threshold: -0.002,
            window: 25,
            invalidation_gain: 0.05,
            volume_must_rise: true,
        }
    }
}

/// How heavy the distribution is.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum DistributionSeverity {
    Normal,
    Caution,
    High,
    Severe,
}

/// The distribution-day count over a series.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct DistributionCount {
    /// Days in the trailing window that are neither expired nor invalidated.
    pub live: usize,
    pub last_5: usize,
    pub last_15: usize,
    pub last_25: usize,
    /// Days cancelled by the rally rule.
    pub invalidated: usize,
    pub severity: DistributionSeverity,
}

/// Distribution days: counting institutional selling into a rally.
///
/// **Source.** William J. O'Neil's CANSLIM market-direction rule, stated in *How
/// to Make Money in Stocks*: a distribution day is an index session that closes
/// down at least about 0.2% on volume higher than the prior session, and the
/// count of such days in the trailing 25 sessions is the pressure gauge. Three
/// to four is caution, five is high, six or more reads as heavy institutional
/// selling. A day is cancelled once the index has since closed 5% above that
/// day's close, because the selling was absorbed.
///
/// **Parameters.** `down_threshold`, `window`, `invalidation_gain` and
/// `volume_must_rise` carry O'Neil's published figures as defaults.
///
/// **Inputs.** The full bar series, oldest first. Stateless: the count is a pure
/// function of the series. Fewer than two bars yields a `Normal` zero count.
pub fn distribution_days(bars: &[IndexBar], p: &DistributionParams) -> DistributionCount {
    let mut count = DistributionCount {
        live: 0,
        last_5: 0,
        last_15: 0,
        last_25: 0,
        invalidated: 0,
        severity: DistributionSeverity::Normal,
    };
    if bars.len() < 2 {
        return count;
    }
    let last = bars.len() - 1;

    for i in 1..bars.len() {
        let (bar, prev) = (bars[i], bars[i - 1]);
        if prev.close <= 0.0 {
            continue;
        }
        let change = (bar.close - prev.close) / prev.close;
        if change > p.down_threshold {
            continue;
        }
        if p.volume_must_rise && bar.volume <= prev.volume {
            continue;
        }

        let sessions_ago = last - i;
        let expired = sessions_ago >= p.window;
        let invalidation_level = bar.close * (1.0 + p.invalidation_gain);
        let invalidated = bars[i + 1..].iter().any(|b| b.close >= invalidation_level);

        if invalidated {
            count.invalidated += 1;
            continue;
        }
        if expired {
            continue;
        }
        count.live += 1;
        if sessions_ago < 5 {
            count.last_5 += 1;
        }
        if sessions_ago < 15 {
            count.last_15 += 1;
        }
        if sessions_ago < 25 {
            count.last_25 += 1;
        }
    }

    count.severity = match count.live {
        0..=2 => DistributionSeverity::Normal,
        3..=4 => DistributionSeverity::Caution,
        5 => DistributionSeverity::High,
        _ => DistributionSeverity::Severe,
    };
    count
}

/// Parameters of the follow-through-day rule.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
pub struct FollowThroughParams {
    /// Earliest rally day on which a follow-through can qualify.
    pub min_rally_day: u32,
    /// Minimum up-close fraction for a follow-through day.
    pub gain_threshold: f64,
}

impl Default for FollowThroughParams {
    /// O'Neil's published figures: day four at the earliest, at least 1.25% up.
    fn default() -> Self {
        Self {
            min_rally_day: 4,
            gain_threshold: 0.0125,
        }
    }
}

/// Where a rally attempt currently stands.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FollowThroughPhase {
    /// No attempt underway and no live confirmation.
    NoRally,
    /// An attempt is underway, not yet confirmed.
    RallyAttempt,
    /// A follow-through day is live and its pivot low still holds.
    ConfirmedUptrend,
    /// A confirmed uptrend was broken by a lower low.
    Undercut,
}

/// The follow-through-day reading over a series.
#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
pub struct FollowThroughSignal {
    pub phase: FollowThroughPhase,
    /// Bar index that began the current attempt, or `None`.
    pub rally_start: Option<usize>,
    /// Rally day count at the end of the series. 0 outside an attempt.
    pub rally_day: u32,
    /// Lowest low of the current attempt: the pivot a confirmation must hold.
    pub rally_low: Option<f64>,
    /// Bar index of the live follow-through day, or `None`.
    pub follow_through: Option<usize>,
    /// Whether any follow-through occurred anywhere in the series, even one
    /// later undercut.
    pub ever_confirmed: bool,
}

/// Follow-through day: the confirmation that a market bottom has taken.
///
/// **Source.** William J. O'Neil's follow-through rule, the companion to the
/// distribution count in *How to Make Money in Stocks*. Day 1 of a rally attempt
/// is the first up close after a new low. From day four onward, a session that
/// closes up at least about 1.25% on volume higher than the prior session
/// confirms the attempt. The confirmation is void the moment the index makes a
/// lower low than the pivot the attempt started from, which is what turns the
/// rule into a state machine rather than a screen.
///
/// **Parameters.** `min_rally_day` and `gain_threshold` carry O'Neil's published
/// figures as defaults.
///
/// **Inputs.** The full bar series, oldest first. Stateless.
pub fn follow_through_day(bars: &[IndexBar], p: &FollowThroughParams) -> FollowThroughSignal {
    let empty = FollowThroughSignal {
        phase: FollowThroughPhase::NoRally,
        rally_start: None,
        rally_day: 0,
        rally_low: None,
        follow_through: None,
        ever_confirmed: false,
    };
    if bars.len() < 2 {
        return empty;
    }

    let mut rally_low = f64::INFINITY;
    let mut rally_low_index: Option<usize> = None;
    let mut in_attempt = false;
    let mut rally_start: Option<usize> = None;
    let mut rally_day: u32 = 0;
    let mut confirmed = false;
    let mut follow_through: Option<usize> = None;
    let mut ever_confirmed = false;
    let mut last_event_undercut = false;

    for i in 0..bars.len() {
        let bar = bars[i];
        if bar.low < rally_low {
            // A fresh low voids any attempt or confirmation and restarts the
            // search for a bottom.
            if confirmed {
                last_event_undercut = true;
            }
            rally_low = bar.low;
            rally_low_index = Some(i);
            in_attempt = false;
            rally_start = None;
            rally_day = 0;
            confirmed = false;
            follow_through = None;
            continue;
        }
        let Some(low_index) = rally_low_index else {
            continue;
        };
        let prev = bars[i - 1];

        if !in_attempt {
            if i > low_index && bar.close > prev.close {
                in_attempt = true;
                rally_start = Some(i);
                rally_day = 1;
                last_event_undercut = false;
            }
            continue;
        }

        rally_day += 1;
        if !confirmed && rally_day >= p.min_rally_day && prev.close > 0.0 {
            let gain = (bar.close - prev.close) / prev.close;
            if gain >= p.gain_threshold && bar.volume > prev.volume {
                confirmed = true;
                ever_confirmed = true;
                last_event_undercut = false;
                follow_through = Some(i);
            }
        }
    }

    let phase = if confirmed {
        FollowThroughPhase::ConfirmedUptrend
    } else if in_attempt {
        FollowThroughPhase::RallyAttempt
    } else if ever_confirmed && last_event_undercut {
        FollowThroughPhase::Undercut
    } else {
        FollowThroughPhase::NoRally
    };

    FollowThroughSignal {
        phase,
        rally_start,
        rally_day: if in_attempt { rally_day } else { 0 },
        rally_low: if rally_low.is_finite() {
            Some(rally_low)
        } else {
            None
        },
        follow_through,
        ever_confirmed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- regime RSI ---------------------------------------------------------

    #[test]
    fn regime_rsi_reads_the_same_number_differently_by_regime() {
        let p = RegimeRsiParams::default();
        // RSI 65 is inside the bull band (35, 75) but above the bear high (60).
        assert_eq!(
            regime_rsi(TrendRegime::Bull, 65.0, 20.0, &p).side,
            Side::None
        );
        assert_eq!(
            regime_rsi(TrendRegime::Bear, 65.0, 20.0, &p).side,
            Side::Short
        );
    }

    #[test]
    fn a_strong_trend_widens_the_overbought_line_only() {
        let p = RegimeRsiParams::default();
        let calm = regime_rsi(TrendRegime::Bull, 78.0, 20.0, &p);
        let strong = regime_rsi(TrendRegime::Bull, 78.0, 40.0, &p);
        assert_eq!(calm.side, Side::Short, "78 clears the unmodulated 75");
        assert_eq!(strong.side, Side::None, "80 is the modulated line");
        assert_eq!(strong.effective_high, 80.0);
        assert_eq!(
            strong.effective_low, 35.0,
            "only one modulation applies at a time"
        );
    }

    #[test]
    fn a_weak_trend_lowers_the_oversold_line_only() {
        let p = RegimeRsiParams::default();
        let weak = regime_rsi(TrendRegime::Bull, 32.0, 10.0, &p);
        assert_eq!(weak.effective_low, 30.0);
        assert_eq!(weak.effective_high, 75.0);
        assert_eq!(weak.side, Side::None, "32 no longer clears the moved line");
        assert_eq!(
            regime_rsi(TrendRegime::Bull, 32.0, 20.0, &p).side,
            Side::Long
        );
    }

    #[test]
    fn idle_uses_the_symmetric_textbook_bands() {
        let p = RegimeRsiParams::default();
        let s = regime_rsi(TrendRegime::Idle, 50.0, 20.0, &p);
        assert_eq!((s.effective_low, s.effective_high), (30.0, 70.0));
        assert_eq!(s.side, Side::None);
    }

    // -- bounce counter -----------------------------------------------------

    /// Feed a series through the counter, returning every emitted side.
    fn run_bounces(series: &[f64], p: &BounceParams) -> Vec<Side> {
        let mut state = BounceState::default();
        series
            .iter()
            .map(|&v| {
                let (side, next) = bounce_counter(state, v, p);
                state = next;
                side
            })
            .collect()
    }

    #[test]
    fn a_single_touch_of_an_extreme_does_not_fire() {
        let p = BounceParams::default();
        let sides = run_bounces(&[75.0, 76.0, 77.0, 78.0, 79.0], &p);
        assert!(
            sides.iter().all(|s| *s == Side::None),
            "one occupancy is one bounce short of the requirement"
        );
    }

    #[test]
    fn the_second_re_entry_fires_once_the_zone_persists() {
        let p = BounceParams::default();
        // High, out to neutral, back to high and held for three bars.
        let sides = run_bounces(&[75.0, 50.0, 75.0, 50.0, 75.0, 76.0, 77.0], &p);
        assert_eq!(sides.iter().filter(|s| **s == Side::Short).count(), 1);
        assert_eq!(
            sides[6],
            Side::Short,
            "fires on the third bar of the second re-entry"
        );
    }

    #[test]
    fn the_counter_fades_the_extreme_it_measures() {
        let p = BounceParams::default();
        let lows = run_bounces(&[25.0, 50.0, 25.0, 50.0, 25.0, 24.0, 23.0], &p);
        assert_eq!(lows[6], Side::Long, "a persisted oversold zone fires long");
    }

    #[test]
    fn a_long_neutral_run_resets_the_bounce_count() {
        let p = BounceParams {
            reset_after_flats: 3,
            ..BounceParams::default()
        };
        let mut series = vec![75.0, 50.0, 75.0];
        series.extend([50.0; 5]); // longer than reset_after_flats
        series.extend([75.0, 76.0, 77.0]);
        let sides = run_bounces(&series, &p);
        assert!(
            sides.iter().all(|s| *s == Side::None),
            "the amnesia window put the count back to one bounce"
        );
    }

    #[test]
    fn the_counter_fires_at_most_once_per_occupancy() {
        let p = BounceParams::default();
        let sides = run_bounces(&[75.0, 50.0, 75.0, 50.0, 75.0, 76.0, 77.0, 78.0, 79.0], &p);
        assert_eq!(sides.iter().filter(|s| **s != Side::None).count(), 1);
    }

    #[test]
    fn the_counter_is_a_pure_function_of_state_and_input() {
        let p = BounceParams::default();
        let series = [75.0, 50.0, 75.0, 50.0, 75.0, 76.0, 77.0];
        assert_eq!(run_bounces(&series, &p), run_bounces(&series, &p));
    }

    // -- signal gate --------------------------------------------------------

    #[test]
    fn an_aligned_signal_goes_straight_through() {
        let (d, s) = signal_gate(GateState::default(), Side::Long, 11.0, 10.0, 100.0, 10);
        assert_eq!(d.execute, Side::Long);
        assert_eq!(d.status, GateStatus::ExecutedImmediately);
        assert_eq!(s, GateState::default(), "nothing is parked");
    }

    #[test]
    fn a_disagreeing_signal_is_parked_and_released_on_confirmation() {
        let (d1, s1) = signal_gate(GateState::default(), Side::Long, 9.0, 10.0, 100.0, 10);
        assert_eq!(d1.status, GateStatus::Pending);
        assert_eq!(s1.pending, Side::Long);
        assert_eq!(s1.pending_price, 100.0);

        let (d2, s2) = signal_gate(s1, Side::None, 11.0, 10.0, 98.0, 10);
        assert_eq!(d2.execute, Side::Long);
        assert_eq!(d2.status, GateStatus::ExecutedAfterConfirmation);
        assert_eq!(
            d2.benefit,
            Some(0.02),
            "a long filled 2% lower is a 2% better fill"
        );
        assert_eq!(s2, GateState::default());
    }

    #[test]
    fn waiting_that_costs_is_reported_as_a_negative_benefit() {
        let (_, s1) = signal_gate(GateState::default(), Side::Long, 9.0, 10.0, 100.0, 10);
        let (d2, _) = signal_gate(s1, Side::None, 11.0, 10.0, 105.0, 10);
        assert_eq!(d2.benefit, Some(-0.05));
    }

    #[test]
    fn a_reversed_signal_cancels_the_parked_one() {
        let (_, s1) = signal_gate(GateState::default(), Side::Long, 9.0, 10.0, 100.0, 10);
        let (d2, s2) = signal_gate(s1, Side::Short, 9.5, 10.0, 99.0, 10);
        assert_eq!(d2.status, GateStatus::Cancelled);
        assert_eq!(d2.execute, Side::None);
        assert_eq!(s2, GateState::default());
    }

    #[test]
    fn a_signal_that_waits_too_long_is_dropped() {
        // The parking call itself is wait 1, so max_wait 3 leaves two more bars.
        let mut state = signal_gate(GateState::default(), Side::Long, 9.0, 10.0, 100.0, 3).1;
        let mut statuses = Vec::new();
        for _ in 0..2 {
            let (d, next) = signal_gate(state, Side::None, 9.0, 10.0, 100.0, 3);
            statuses.push(d.status);
            state = next;
        }
        assert_eq!(statuses, vec![GateStatus::Pending, GateStatus::Cancelled]);
        assert_eq!(state, GateState::default(), "the stale signal is dropped");
    }

    #[test]
    fn no_signal_passes_through_untouched() {
        let (d, s) = signal_gate(GateState::default(), Side::None, 9.0, 10.0, 100.0, 10);
        assert_eq!(d.status, GateStatus::Passthrough);
        assert_eq!(s, GateState::default());
    }

    // -- time stop ----------------------------------------------------------

    #[test]
    fn the_time_stop_forces_an_exit_at_the_bar_limit() {
        let mut state = ExposureState::default();
        let (entry, next) = max_exposure_timeout(state, Side::Long, 100.0, 3);
        assert_eq!(entry.action, Side::Long);
        state = next;

        let mut forced = None;
        for bar in 0..3 {
            let (d, next) = max_exposure_timeout(state, Side::None, 101.0 + bar as f64, 3);
            state = next;
            if d.forced_exit {
                forced = Some(d);
                break;
            }
        }
        let d = forced.expect("the clock must eventually close the position");
        assert_eq!(d.action, Side::Short, "a forced exit sells the long");
        assert_eq!(state, ExposureState::default());
    }

    #[test]
    fn an_opposing_signal_closes_without_being_called_forced() {
        let (_, state) = max_exposure_timeout(ExposureState::default(), Side::Long, 100.0, 50);
        let (d, next) = max_exposure_timeout(state, Side::Short, 99.0, 50);
        assert_eq!(d.action, Side::Short);
        assert!(!d.forced_exit, "a thesis change is not a time stop");
        assert_eq!(next, ExposureState::default());
    }

    #[test]
    fn a_same_side_signal_holds_and_advances_the_clock() {
        let (_, s0) = max_exposure_timeout(ExposureState::default(), Side::Long, 100.0, 50);
        let (d, s1) = max_exposure_timeout(s0, Side::Long, 101.0, 50);
        assert_eq!(d.action, Side::None);
        assert_eq!(s1.bars_held, 1);
        assert_eq!(s1.entry_price, 100.0, "the entry price is not restamped");
    }

    #[test]
    fn flat_with_no_signal_stays_flat() {
        let (d, s) = max_exposure_timeout(ExposureState::default(), Side::None, 100.0, 5);
        assert_eq!(d.action, Side::None);
        assert!(!d.forced_exit);
        assert_eq!(s, ExposureState::default());
    }

    // -- ATR breakout -------------------------------------------------------

    #[test]
    fn expanding_volatility_arms_symmetric_stops() {
        let p = AtrBreakoutParams::default();
        let s = atr_breakout(100.0, 3.0, 2.0, &p);
        assert_eq!(s.verdict, AtrBreakoutVerdict::Armed);
        let l = s.levels.expect("armed carries levels");
        assert_eq!(l.long_entry_stop, 105.0);
        assert_eq!(l.short_entry_stop, 95.0);
        assert_eq!(l.exit_stop_distance, 2.0);
        assert_eq!(l.long_stop_loss, 103.0);
        assert_eq!(l.short_stop_loss, 97.0);
    }

    #[test]
    fn contracting_volatility_does_not_arm() {
        let p = AtrBreakoutParams::default();
        let s = atr_breakout(100.0, 1.5, 2.0, &p);
        assert_eq!(s.verdict, AtrBreakoutVerdict::FilteredNoExpansion);
        assert_eq!(s.levels, None);
        assert_eq!(s.expansion_ratio, 0.75);
    }

    #[test]
    fn a_quiet_market_is_filtered_before_the_expansion_test() {
        let p = AtrBreakoutParams {
            min_atr_long: 5.0,
            ..AtrBreakoutParams::default()
        };
        assert_eq!(
            atr_breakout(100.0, 9.0, 2.0, &p).verdict,
            AtrBreakoutVerdict::FilteredLowVol,
            "expanding but tiny is still untradeably quiet"
        );
    }

    #[test]
    fn non_finite_or_zero_atr_is_rejected_rather_than_propagated() {
        let p = AtrBreakoutParams::default();
        for (o, short, long) in [
            (f64::NAN, 3.0, 2.0),
            (100.0, f64::INFINITY, 2.0),
            (100.0, 3.0, 0.0),
            (100.0, -1.0, 2.0),
        ] {
            let s = atr_breakout(o, short, long, &p);
            assert_eq!(s.verdict, AtrBreakoutVerdict::InvalidInput);
            assert_eq!(s.levels, None);
        }
    }

    #[test]
    fn the_exit_stop_is_tighter_than_the_entry_distance_by_default() {
        let p = AtrBreakoutParams::default();
        assert!(p.exit_multiplier < p.entry_multiplier);
    }

    // -- distribution days --------------------------------------------------

    fn bar(close: f64, volume: f64) -> IndexBar {
        IndexBar {
            high: close,
            low: close,
            close,
            volume,
        }
    }

    #[test]
    fn a_down_close_on_rising_volume_counts() {
        let bars = [bar(100.0, 1000.0), bar(99.0, 1200.0)];
        let c = distribution_days(&bars, &DistributionParams::default());
        assert_eq!(c.live, 1);
        assert_eq!(c.last_5, 1);
        assert_eq!(c.severity, DistributionSeverity::Normal);
    }

    #[test]
    fn a_down_close_on_falling_volume_does_not_count() {
        let bars = [bar(100.0, 1000.0), bar(99.0, 800.0)];
        assert_eq!(
            distribution_days(&bars, &DistributionParams::default()).live,
            0
        );
    }

    #[test]
    fn a_shallow_down_close_does_not_count() {
        // -0.1% is above the -0.2% threshold.
        let bars = [bar(100.0, 1000.0), bar(99.9, 1200.0)];
        assert_eq!(
            distribution_days(&bars, &DistributionParams::default()).live,
            0
        );
    }

    #[test]
    fn six_live_days_read_as_severe() {
        let mut bars = vec![bar(100.0, 1000.0)];
        let mut close = 100.0;
        for _ in 0..6 {
            close -= 1.0;
            bars.push(bar(close, 2000.0));
            bars.push(bar(close + 0.01, 1000.0)); // a flat-ish up bar on low volume
        }
        let c = distribution_days(&bars, &DistributionParams::default());
        assert_eq!(c.live, 6, "{c:?}");
        assert_eq!(c.severity, DistributionSeverity::Severe);
    }

    #[test]
    fn the_rally_rule_cancels_an_absorbed_day() {
        // One distribution day, then a rally of more than 5% above its close.
        let bars = [
            bar(100.0, 1000.0),
            bar(99.0, 1200.0),
            bar(105.0, 900.0), // 105 >= 99 * 1.05 = 103.95
        ];
        let c = distribution_days(&bars, &DistributionParams::default());
        assert_eq!(c.live, 0);
        assert_eq!(c.invalidated, 1);
    }

    #[test]
    fn a_day_outside_the_window_expires() {
        let p = DistributionParams {
            window: 2,
            ..DistributionParams::default()
        };
        let bars = [
            bar(100.0, 1000.0),
            bar(99.0, 1200.0), // the distribution day
            bar(99.1, 900.0),
            bar(99.2, 900.0),
            bar(99.3, 900.0),
        ];
        let c = distribution_days(&bars, &p);
        assert_eq!(c.live, 0, "aged out of a two-session window");
        assert_eq!(c.invalidated, 0, "expiry is not invalidation");
    }

    #[test]
    fn too_few_bars_is_a_normal_zero_count() {
        let c = distribution_days(&[bar(100.0, 1.0)], &DistributionParams::default());
        assert_eq!(c.live, 0);
        assert_eq!(c.severity, DistributionSeverity::Normal);
    }

    // -- follow-through day -------------------------------------------------

    fn ohlc(low: f64, close: f64, volume: f64) -> IndexBar {
        IndexBar {
            high: close,
            low,
            close,
            volume,
        }
    }

    #[test]
    fn a_qualifying_day_four_confirms_the_attempt() {
        let bars = [
            ohlc(90.0, 91.0, 1000.0), // the low
            ohlc(91.0, 92.0, 1000.0), // day 1: first up close
            ohlc(91.5, 92.5, 1000.0), // day 2
            ohlc(92.0, 93.0, 1000.0), // day 3
            ohlc(92.5, 95.0, 2000.0), // day 4: +2.15% on double volume
        ];
        let s = follow_through_day(&bars, &FollowThroughParams::default());
        assert_eq!(s.phase, FollowThroughPhase::ConfirmedUptrend);
        assert_eq!(s.follow_through, Some(4));
        assert!(s.ever_confirmed);
        assert_eq!(s.rally_start, Some(1));
    }

    #[test]
    fn a_day_three_gain_is_too_early_to_confirm() {
        let bars = [
            ohlc(90.0, 91.0, 1000.0),
            ohlc(91.0, 92.0, 1000.0), // day 1
            ohlc(91.5, 92.5, 1000.0), // day 2
            ohlc(92.0, 95.0, 3000.0), // day 3: big, but too early
        ];
        let s = follow_through_day(&bars, &FollowThroughParams::default());
        assert_eq!(s.phase, FollowThroughPhase::RallyAttempt);
        assert_eq!(s.follow_through, None);
        assert!(!s.ever_confirmed);
    }

    #[test]
    fn a_qualifying_gain_on_falling_volume_does_not_confirm() {
        let bars = [
            ohlc(90.0, 91.0, 3000.0),
            ohlc(91.0, 92.0, 2000.0),
            ohlc(91.5, 92.5, 2000.0),
            ohlc(92.0, 93.0, 2000.0),
            ohlc(92.5, 95.0, 1000.0), // +2.15% but volume fell
        ];
        assert_eq!(
            follow_through_day(&bars, &FollowThroughParams::default()).phase,
            FollowThroughPhase::RallyAttempt
        );
    }

    #[test]
    fn a_lower_low_undercuts_a_confirmed_uptrend() {
        let bars = [
            ohlc(90.0, 91.0, 1000.0),
            ohlc(91.0, 92.0, 1000.0),
            ohlc(91.5, 92.5, 1000.0),
            ohlc(92.0, 93.0, 1000.0),
            ohlc(92.5, 95.0, 2000.0), // confirmed here
            ohlc(85.0, 86.0, 1500.0), // a new low voids it
        ];
        let s = follow_through_day(&bars, &FollowThroughParams::default());
        assert_eq!(s.phase, FollowThroughPhase::Undercut);
        assert!(s.ever_confirmed, "the event still happened");
        assert_eq!(s.follow_through, None, "but it is no longer live");
        assert_eq!(s.rally_low, Some(85.0));
    }

    #[test]
    fn a_series_that_only_falls_never_starts_an_attempt() {
        let bars = [
            ohlc(100.0, 100.0, 1000.0),
            ohlc(99.0, 99.0, 1000.0),
            ohlc(98.0, 98.0, 1000.0),
        ];
        let s = follow_through_day(&bars, &FollowThroughParams::default());
        assert_eq!(s.phase, FollowThroughPhase::NoRally);
        assert_eq!(s.rally_start, None);
        assert_eq!(s.rally_day, 0);
    }

    #[test]
    fn too_few_bars_is_no_rally() {
        let s = follow_through_day(&[ohlc(1.0, 1.0, 1.0)], &FollowThroughParams::default());
        assert_eq!(s.phase, FollowThroughPhase::NoRally);
        assert_eq!(s.rally_low, None);
    }

    // -- house rules --------------------------------------------------------

    #[test]
    fn every_stateful_primitive_replays_identically_from_the_same_state() {
        // The property the kernel needs: no hidden state anywhere.
        let bp = BounceParams::default();
        let series = [75.0, 50.0, 75.0, 50.0, 75.0, 76.0, 77.0, 40.0, 25.0];
        let replay = |_: ()| {
            let mut bounce = BounceState::default();
            let mut gate = GateState::default();
            let mut exposure = ExposureState::default();
            let mut out = Vec::new();
            for (i, &v) in series.iter().enumerate() {
                let (side, nb) = bounce_counter(bounce, v, &bp);
                bounce = nb;
                let price = 100.0 + i as f64;
                let (gd, ng) = signal_gate(gate, side, v, 60.0, price, 5);
                gate = ng;
                let (ed, ne) = max_exposure_timeout(exposure, gd.execute, price, 4);
                exposure = ne;
                out.push((side, gd.status, ed.action, ed.forced_exit));
            }
            out
        };
        assert_eq!(replay(()), replay(()));
    }
}
