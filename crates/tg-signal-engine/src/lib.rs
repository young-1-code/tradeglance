#![forbid(unsafe_code)]

pub mod grpc;
pub mod rules;
pub mod sink;
pub mod sources;
pub mod strategies;

pub use grpc::SignalGrpcService;
pub use rules::{
    CmpOp, Condition, ConditionTree, EvalContext, PriceField, Rule, RuleEvaluation,
};
pub use sink::{BroadcastSignalSink, NoopSink, SignalCollector, SignalSink};
pub use sources::{
    FactorSource, FactorValueRequest, GrpcFactorSource, GrpcIndicatorSource, IndicatorSeries,
    IndicatorSeriesRequest, IndicatorSource, InMemoryFactorSource, InMemoryIndicatorSource,
};
pub use strategies::{LimitUpStrategy, SwingStrategy, T0Strategy};
