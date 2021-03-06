#[macro_use]
extern crate abomonation;
extern crate timely;
extern crate differential_dataflow;
extern crate arrayvec;
extern crate regex;

use timely::dataflow::*;

use differential_dataflow::Collection;

pub mod types;
pub mod queries;

pub use types::*;

pub struct Collections<G: Scope> {
    customers: Collection<G, Customer, isize>,
    lineitems: Collection<G, LineItem, isize>,
    nations: Collection<G, Nation, isize>,
    orders: Collection<G, Order, isize>,
    parts: Collection<G, Part, isize>,
    partsupps: Collection<G, PartSupp, isize>,
    regions: Collection<G, Region, isize>,
    suppliers: Collection<G, Supplier, isize>,
    used: [bool; 8],
}

impl<G: Scope> Collections<G> {
    pub fn new(
        customers: Collection<G, Customer, isize>,
        lineitems: Collection<G, LineItem, isize>,
        nations: Collection<G, Nation, isize>,
        orders: Collection<G, Order, isize>,
        parts: Collection<G, Part, isize>,
        partsupps: Collection<G, PartSupp, isize>,
        regions: Collection<G, Region, isize>,
        suppliers: Collection<G, Supplier, isize>,
    ) -> Self {

        Collections {
            customers: customers,
            lineitems: lineitems,
            nations: nations,
            orders: orders,
            parts: parts,
            partsupps: partsupps,
            regions: regions,
            suppliers: suppliers,
            used: [false; 8]
        }
    }

    pub fn customers(&mut self) -> &Collection<G, Customer, isize> { self.used[0] = true; &self.customers }
    pub fn lineitems(&mut self) -> &Collection<G, LineItem, isize> { self.used[1] = true; &self.lineitems }
    pub fn nations(&mut self) -> &Collection<G, Nation, isize> { self.used[2] = true; &self.nations }
    pub fn orders(&mut self) -> &Collection<G, Order, isize> { self.used[3] = true; &self.orders }
    pub fn parts(&mut self) -> &Collection<G, Part, isize> { self.used[4] = true; &self.parts }
    pub fn partsupps(&mut self) -> &Collection<G, PartSupp, isize> { self.used[5] = true; &self.partsupps }
    pub fn regions(&mut self) -> &Collection<G, Region, isize> { self.used[6] = true; &self.regions }
    pub fn suppliers(&mut self) -> &Collection<G, Supplier, isize> { self.used[7] = true; &self.suppliers }

    pub fn used(&self) -> [bool; 8] { self.used }
}


use differential_dataflow::trace::implementations::ord::OrdValSpine as DefaultValTrace;
use differential_dataflow::operators::arrange::TraceAgent;

type ArrangedIndex<T> = TraceAgent<usize, T, usize, isize, DefaultValTrace<usize, T, usize, isize>>;

pub struct Arrangements {
    customers:  ArrangedIndex<Customer>,
    nations:    ArrangedIndex<Nation>,
    orders:     ArrangedIndex<Order>,
    parts:      ArrangedIndex<Part>,
    regions:    ArrangedIndex<Region>,
    suppliers:  ArrangedIndex<Supplier>,
}

use timely::dataflow::Scope;

impl Arrangements {

    pub fn new<G: Scope<Timestamp=usize>>(collections: &mut Collections<G>, probe: &mut ProbeHandle<usize>) -> Self {

        use timely::dataflow::operators::Probe;
        use differential_dataflow::operators::arrange::ArrangeByKey;
        use differential_dataflow::trace::TraceReader;

        let mut arranged = collections.customers().map(|x| (x.cust_key, x)).arrange_by_key();
        arranged.stream.probe_with(probe);
        arranged.trace.distinguish_since(&[]);
        let customers = arranged.trace;

        let mut arranged = collections.nations().map(|x| (x.nation_key, x)).arrange_by_key();
        arranged.stream.probe_with(probe);
        arranged.trace.distinguish_since(&[]);
        let nations = arranged.trace;

        let mut arranged = collections.orders().map(|x| (x.order_key, x)).arrange_by_key();
        arranged.stream.probe_with(probe);
        arranged.trace.distinguish_since(&[]);
        let orders = arranged.trace;

        let mut arranged = collections.parts().map(|x| (x.part_key, x)).arrange_by_key();
        arranged.stream.probe_with(probe);
        arranged.trace.distinguish_since(&[]);
        let parts = arranged.trace;

        let mut arranged = collections.regions().map(|x| (x.region_key, x)).arrange_by_key();
        arranged.stream.probe_with(probe);
        arranged.trace.distinguish_since(&[]);
        let regions = arranged.trace;

        let mut arranged = collections.suppliers().map(|x| (x.supp_key, x)).arrange_by_key();
        arranged.stream.probe_with(probe);
        arranged.trace.distinguish_since(&[]);
        let suppliers = arranged.trace;

        Arrangements {
            customers,
            nations,
            orders,
            parts,
            regions,
            suppliers,
        }
    }

    pub fn advance_by(&mut self, frontier: &[usize]) {

        use differential_dataflow::trace::TraceReader;

        self.customers.advance_by(frontier);
        self.nations.advance_by(frontier);
        self.orders.advance_by(frontier);
        self.parts.advance_by(frontier);
        self.regions.advance_by(frontier);
        self.suppliers.advance_by(frontier);
    }
}