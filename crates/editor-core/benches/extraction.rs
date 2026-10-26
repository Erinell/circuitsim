//! Criterion benchmarks for schematic connectivity extraction.

use std::hint::black_box;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use editor_core::{
    extract_netlist, Command, GridPos, Orientation, Schematic, Symbol, SymbolId, SymbolKind,
    SymbolRegistry, UndoStack, Wire, WireId,
};

fn add_symbol(
    schematic: &mut Schematic,
    history: &mut UndoStack,
    registry: &SymbolRegistry,
    kind: SymbolKind,
    at: GridPos,
    label: String,
) {
    let definition = registry.get(&kind).expect("symbol is registered").clone();
    let id = SymbolId(schematic.allocate_id().expect("identifier space"));
    let symbol = Symbol::from_def(id, &definition, at, label, Orientation::R0, false);
    history
        .execute(schematic, Command::AddSymbol(symbol))
        .expect("symbol is valid");
}

fn add_wire(schematic: &mut Schematic, history: &mut UndoStack, a: GridPos, b: GridPos) {
    let id = WireId(schematic.allocate_id().expect("identifier space"));
    let wire = Wire::new(id, a, b).expect("wire is orthogonal and non-empty");
    history
        .execute(schematic, Command::AddWire(wire))
        .expect("wire is valid");
}

fn resistor_ladder(count: usize) -> (Schematic, SymbolRegistry) {
    let registry = SymbolRegistry::builtin();
    let mut schematic = Schematic::new();
    let mut history = UndoStack::new();

    add_symbol(
        &mut schematic,
        &mut history,
        &registry,
        SymbolKind::Ground,
        GridPos::new(-2, 2),
        "GND1".to_owned(),
    );
    add_wire(
        &mut schematic,
        &mut history,
        GridPos::new(-2, 0),
        GridPos::new(-2, 2),
    );

    for index in 0..count {
        let x = i32::try_from(index).expect("benchmark size fits the grid") * 8;
        add_symbol(
            &mut schematic,
            &mut history,
            &registry,
            SymbolKind::Resistor,
            GridPos::new(x, 0),
            format!("R{}", index + 1),
        );
        if index + 1 < count {
            add_wire(
                &mut schematic,
                &mut history,
                GridPos::new(x + 2, 0),
                GridPos::new(x + 6, 0),
            );
        }
    }

    (schematic, registry)
}

fn benchmark_extraction(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("extraction");
    for count in [16, 64, 256] {
        let (schematic, registry) = resistor_ladder(count);
        group.bench_with_input(
            BenchmarkId::new("resistor_ladder", count),
            &count,
            |bencher, _| {
                bencher.iter(|| {
                    let extraction = extract_netlist(black_box(&schematic), black_box(&registry))
                        .expect("ladder extraction succeeds");
                    black_box(extraction);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_extraction);
criterion_main!(benches);
