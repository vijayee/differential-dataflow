//! Management of inputs and traces.

use std::collections::HashMap;
use std::hash::Hash;
use std::time::Duration;

use timely::dataflow::ProbeHandle;
use timely::communication::Allocate;
use timely::worker::Worker;
use timely::logging::TimelyEvent;

use timely::dataflow::operators::capture::event::EventIterator;

use differential_dataflow::Data;
use differential_dataflow::trace::implementations::ord::{OrdKeySpine, OrdValSpine};
use differential_dataflow::operators::arrange::TraceAgent;
use differential_dataflow::input::InputSession;

use differential_dataflow::logging::DifferentialEvent;

use super::{Time, Diff, Plan};

/// A trace handle for key-only data.
pub type TraceKeyHandle<K, T, R> = TraceAgent<K, (), T, R, OrdKeySpine<K, T, R>>;
/// A trace handle for key-value data.
pub type TraceValHandle<K, V, T, R> = TraceAgent<K, V, T, R, OrdValSpine<K, V, T, R>>;
/// A key-only trace handle binding `Time` and `Diff` using `Vec<V>` as data.
pub type KeysOnlyHandle<V> = TraceKeyHandle<Vec<V>, Time, Diff>;
/// A key-value trace handle binding `Time` and `Diff` using `Vec<V>` as data.
pub type KeysValsHandle<V> = TraceValHandle<Vec<V>, Vec<V>, Time, Diff>;

/// A type that can be converted to a vector of another type.
pub trait AsVector<T> {
    /// Converts `self` to a vector of `T`.
    fn as_vector(self) -> Vec<T>;
}

/// Manages inputs and traces.
pub struct Manager<Value: Data> {
    /// Manages input sessions.
    pub inputs: InputManager<Value>,
    /// Manages maintained traces.
    pub traces: TraceManager<Value>,
    /// Probes all computations.
    pub probe: ProbeHandle<Time>,
}

impl<Value: Data+Hash> Manager<Value> {

    /// Creates a new empty manager.
    pub fn new() -> Self {
        Manager {
            inputs: InputManager::new(),
            traces: TraceManager::new(),
            probe: ProbeHandle::new(),
        }
    }

    /// Clear the managed inputs and traces.
    pub fn shutdown(&mut self) {
        self.inputs.sessions.clear();
        self.traces.inputs.clear();
        self.traces.arrangements.clear();
    }

    /// Inserts a new input session by name.
    pub fn insert_input(
        &mut self,
        name: String,
        input: InputSession<Time, Vec<Value>, Diff>,
        trace: KeysOnlyHandle<Value>)
    {
        self.inputs.sessions.insert(name.clone(), input);
        self.traces.set_unkeyed(&Plan::Source(name), &trace);
    }

    /// Advances inputs and traces to `time`.
    pub fn advance_time(&mut self, time: &Time) {
        self.inputs.advance_time(time);
        self.traces.advance_time(time);
    }

    /// Timely logging capture and arrangement.
    pub fn publish_timely_logging<A, I>(&mut self, worker: &mut Worker<A>, events: I)
    where
        A: Allocate,
        TimelyEvent: AsVector<Value>,
        I : IntoIterator,
        <I as IntoIterator>::Item: EventIterator<Duration, (Duration, usize, TimelyEvent)>+'static
    {
        let (operates, channels, schedule, messages) =
        worker.dataflow(move |scope| {

            use timely::dataflow::operators::capture::Replay;
            use timely::dataflow::operators::generic::builder_rc::OperatorBuilder;

            let input = events.replay_into(scope);

            let mut demux = OperatorBuilder::new("Timely Logging Demux".to_string(), scope.clone());

            use timely::dataflow::channels::pact::Pipeline;
            let mut input = demux.new_input(&input, Pipeline);

            let (mut operates_out, operates) = demux.new_output();
            let (mut channels_out, channels) = demux.new_output();
            let (mut schedule_out, schedule) = demux.new_output();
            let (mut messages_out, messages) = demux.new_output();

            let mut demux_buffer = Vec::new();

            demux.build(move |_capability| {

                move |_frontiers| {

                    let mut operates = operates_out.activate();
                    let mut channels = channels_out.activate();
                    let mut schedule = schedule_out.activate();
                    let mut messages = messages_out.activate();

                    input.for_each(|time, data| {
                        data.swap(&mut demux_buffer);
                        let mut operates_session = operates.session(&time);
                        let mut channels_session = channels.session(&time);
                        let mut schedule_session = schedule.session(&time);
                        let mut messages_session = messages.session(&time);

                        for (time, _worker, datum) in demux_buffer.drain(..) {
                            match datum {
                                TimelyEvent::Operates(_) => {
                                    operates_session.give((datum.as_vector(), time, 1));
                                },
                                TimelyEvent::Channels(_) => {
                                    channels_session.give((datum.as_vector(), time, 1));
                                },
                                TimelyEvent::Schedule(_) => {
                                    schedule_session.give((datum.as_vector(), time, 1));
                                },
                                TimelyEvent::Messages(_) => {
                                    messages_session.give((datum.as_vector(), time, 1));
                                },
                                _ => { },
                            }
                        }
                    });
                }
            });

            use differential_dataflow::collection::AsCollection;
            use differential_dataflow::operators::arrange::ArrangeBySelf;
            let operates = operates.as_collection().arrange_by_self().trace;
            let channels = channels.as_collection().arrange_by_self().trace;
            let schedule = schedule.as_collection().arrange_by_self().trace;
            let messages = messages.as_collection().arrange_by_self().trace;

            (operates, channels, schedule, messages)
        });

        self.traces.set_unkeyed(&Plan::Source("logs/timely/operates".to_string()), &operates);
        self.traces.set_unkeyed(&Plan::Source("logs/timely/channels".to_string()), &channels);
        self.traces.set_unkeyed(&Plan::Source("logs/timely/schedule".to_string()), &schedule);
        self.traces.set_unkeyed(&Plan::Source("logs/timely/messages".to_string()), &messages);
    }

    /// Timely logging capture and arrangement.
    pub fn publish_differential_logging<A, I>(&mut self, worker: &mut Worker<A>, events: I)
    where
        A: Allocate,
        DifferentialEvent: AsVector<Value>,
        I : IntoIterator,
        <I as IntoIterator>::Item: EventIterator<Duration, (Duration, usize, DifferentialEvent)>+'static
    {
        let (merge,batch) =
        worker.dataflow(move |scope| {

            use timely::dataflow::operators::capture::Replay;
            use timely::dataflow::operators::generic::builder_rc::OperatorBuilder;

            let input = events.replay_into(scope);

            let mut demux = OperatorBuilder::new("Differential Logging Demux".to_string(), scope.clone());

            use timely::dataflow::channels::pact::Pipeline;
            let mut input = demux.new_input(&input, Pipeline);

            let (mut batch_out, batch) = demux.new_output();
            let (mut merge_out, merge) = demux.new_output();

            let mut demux_buffer = Vec::new();

            demux.build(move |_capability| {

                move |_frontiers| {

                    let mut batch = batch_out.activate();
                    let mut merge = merge_out.activate();

                    input.for_each(|time, data| {
                        data.swap(&mut demux_buffer);
                        let mut batch_session = batch.session(&time);
                        let mut merge_session = merge.session(&time);

                        for (time, _worker, datum) in demux_buffer.drain(..) {
                            match datum {
                                DifferentialEvent::Batch(_) => {
                                    batch_session.give((datum.as_vector(), time, 1));
                                },
                                DifferentialEvent::Merge(_) => {
                                    merge_session.give((datum.as_vector(), time, 1));
                                },
                                _ => { },
                            }
                        }
                    });
                }
            });

            use differential_dataflow::collection::AsCollection;
            use differential_dataflow::operators::arrange::ArrangeBySelf;
            let batch = batch.as_collection().arrange_by_self().trace;
            let merge = merge.as_collection().arrange_by_self().trace;

            (merge,batch)
        });

        self.traces.set_unkeyed(&Plan::Source("logs/differential/arrange/batch".to_string()), &batch);
        self.traces.set_unkeyed(&Plan::Source("logs/differential/arrange/merge".to_string()), &merge);
    }
}

/// Manages input sessions.
pub struct InputManager<Value: Data> {
    /// Input sessions by name.
    pub sessions: HashMap<String, InputSession<Time, Vec<Value>, Diff>>,
}

impl<Value: Data> InputManager<Value> {

    /// Creates a new empty input manager.
    pub fn new() -> Self { Self { sessions: HashMap::new() } }

    /// Advances the times of all managed inputs.
    pub fn advance_time(&mut self, time: &Time) {
        for session in self.sessions.values_mut() {
            session.advance_to(time.clone());
            session.flush();
        }
    }

}

/// Root handles to maintained collections.
///
/// Manages a map from plan (describing a collection)
/// to various arranged forms of that collection.
pub struct TraceManager<Value: Data> {

    /// Arrangements where the record itself is they key.
    ///
    /// This contains both input collections, which are here cached so that
    /// they can be re-used, intermediate collections that are cached, and
    /// any collections that are explicitly published.
    inputs: HashMap<Plan<Value>, KeysOnlyHandle<Value>>,

    /// Arrangements of collections by key.
    arrangements: HashMap<Plan<Value>, HashMap<Vec<usize>, KeysValsHandle<Value>>>,

}

impl<Value: Data+Hash> TraceManager<Value> {

    /// Creates a new empty trace manager.
    pub fn new() -> Self { Self { inputs: HashMap::new(), arrangements: HashMap::new() } }

    /// Advances the frontier of each maintained trace.
    pub fn advance_time(&mut self, time: &Time) {
        use differential_dataflow::trace::TraceReader;

        let frontier = &[time.clone()];
        for trace in self.inputs.values_mut() {
            trace.advance_by(frontier);
        }
        for map in self.arrangements.values_mut() {
            for trace in map.values_mut() {
                trace.advance_by(frontier)
            }
        }
    }

    /// Recover an arrangement by plan and keys, if it is cached.
    pub fn get_unkeyed(&self, plan: &Plan<Value>) -> Option<KeysOnlyHandle<Value>> {
        self.inputs
            .get(plan)
            .map(|x| x.clone())
    }

    /// Installs an unkeyed arrangement for a specified plan.
    pub fn set_unkeyed(&mut self, plan: &Plan<Value>, handle: &KeysOnlyHandle<Value>) {

        println!("Setting unkeyed: {:?}", plan);

        use differential_dataflow::trace::TraceReader;
        let mut handle = handle.clone();
        handle.distinguish_since(&[]);
        self.inputs
            .insert(plan.clone(), handle);
    }

    /// Recover an arrangement by plan and keys, if it is cached.
    pub fn get_keyed(&self, plan: &Plan<Value>, keys: &[usize]) -> Option<KeysValsHandle<Value>> {
        self.arrangements
            .get(plan)
            .and_then(|map| map.get(keys).map(|x| x.clone()))
    }

    /// Installs a keyed arrangement for a specified plan and sequence of keys.
    pub fn set_keyed(&mut self, plan: &Plan<Value>, keys: &[usize], handle: &KeysValsHandle<Value>) {
        use differential_dataflow::trace::TraceReader;
        let mut handle = handle.clone();
        handle.distinguish_since(&[]);
        self.arrangements
            .entry(plan.clone())
            .or_insert(HashMap::new())
            .insert(keys.to_vec(), handle);
    }

}