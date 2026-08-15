use crate::common::Bench;

pub(crate) trait BenchConstruct: Bench {
    /// The full construction.
    fn construct_full(entries: Vec<(Self::Key, usize)>) -> impl Sized;

    /// The perfect-hash search alone.
    fn construct_search(entries: Vec<(Self::Key, usize)>) -> impl Sized;
}

/// Registers the construction benchmarks for one adapter type.
macro_rules! construct_benches {
    ($adapter:ident) => {
        $crate::common_construct::construct_benches!(@one $adapter,
            /// The full construction.
            construct_full, construct_full);
        $crate::common_construct::construct_benches!(@one $adapter,
            /// The perfect-hash search alone.
            construct_search, construct_search);
    };
    (@one $adapter:ident, $(#[$doc:meta])* $name:ident, $method:ident) => {
        $(#[$doc])*
        #[divan::bench(
            types = [
                $adapter<$crate::common::Ints>,
                $adapter<$crate::common::ShortStr>,
                $adapter<$crate::common::LongStr>,
            ],
            args = $crate::common::SIZES,
        )]
        fn $name<C: $crate::common_construct::BenchConstruct>(
            bencher: ::divan::Bencher<'_, '_>,
            n: usize,
        ) {
            let entries: Vec<_> = C::present(n).into_iter().zip(0usize..).collect();
            bencher
                .with_inputs(|| entries.clone())
                .bench_values(|entries| C::$method(entries));
        }
    };
}
pub(crate) use construct_benches;
