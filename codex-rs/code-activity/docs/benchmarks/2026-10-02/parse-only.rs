//! Standalone diagnostic copied into a pinned baseline's examples directory.
//! It parses inert Python strings; it never executes source or accesses targets.

use std::hint::black_box;
use std::io::Write;
use std::time::Duration;
use std::time::Instant;
use tree_sitter::Parser;

const PREFIX: &str = "open(\"input.txt\").read()\n";
const COMMENT: &str = "# padded source without extra actions\n";
const WARMUP: usize = 1;
const ITERATIONS: usize = 3;

#[derive(Clone, Copy, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Metadata {
    named_root_children: usize,
    syntax_error: bool,
}

fn parse_and_drop(parser: &mut Parser, source: &str) -> eyre::Result<(Duration, Metadata)> {
    let started = Instant::now();
    let tree = black_box(parser.parse(black_box(source.as_bytes()), None))
        .ok_or_else(|| eyre::eyre!("native parse interrupted"))?;
    let metadata = Metadata {
        named_root_children: tree.root_node().named_child_count(),
        syntax_error: tree.root_node().has_error(),
    };
    drop(black_box(tree));
    Ok((started.elapsed(), metadata))
}

fn collect_root_children(tree: &tree_sitter::Tree) -> usize {
    let root = black_box(tree.root_node());
    let mut cursor = root.walk();
    let children: Vec<_> = root.named_children(&mut cursor).collect();
    let count = black_box(children.len());
    drop(black_box(children));
    count
}

fn timings(samples: &[Duration; ITERATIONS]) -> serde_json::Value {
    let raw = samples.map(|sample| sample.as_secs_f64() * 1e6);
    let mut ordered = raw;
    ordered.sort_by(f64::total_cmp);
    let mean = raw.iter().sum::<f64>() / 3.0;
    let variance = raw
        .iter()
        .map(|sample| (sample - mean).powi(2))
        .sum::<f64>()
        / 3.0;
    serde_json::json!({
        "raw": raw,
        "min": ordered[0], "p50": ordered[1], "p95": ordered[2], "max": ordered[2],
        "mean": mean, "populationStddev": variance.sqrt(),
    })
}

fn main() -> eyre::Result<()> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    let mut stdout = std::io::stdout().lock();
    for count in [1024, 8192] {
        let source = format!("{PREFIX}{}", COMMENT.repeat(count));
        for _ in 0..WARMUP {
            black_box(parse_and_drop(&mut parser, &source)?);
        }
        let mut parse_samples = [Duration::ZERO; ITERATIONS];
        let mut metadata = Metadata {
            named_root_children: 0,
            syntax_error: false,
        };
        for sample in &mut parse_samples {
            let (elapsed, observed) = parse_and_drop(&mut parser, &source)?;
            *sample = elapsed;
            metadata = observed;
        }

        // Reuse one independently parsed tree to separate traversal from parsing.
        let tree = parser
            .parse(source.as_bytes(), None)
            .ok_or_else(|| eyre::eyre!("native parse interrupted during traversal setup"))?;
        for _ in 0..WARMUP {
            black_box(collect_root_children(&tree));
        }
        let mut traversal_samples = [Duration::ZERO; ITERATIONS];
        for sample in &mut traversal_samples {
            let started = Instant::now();
            let observed = black_box(collect_root_children(&tree));
            *sample = started.elapsed();
            eyre::ensure!(
                observed == metadata.named_root_children,
                "root child count changed"
            );
        }
        serde_json::to_writer(
            &mut stdout,
            &serde_json::json!({
                "diagnosticVersion": 1, "datasetVersion": 1,
                "case": format!("python-padding-{count}"), "language": "python",
                "inputBytes": source.len(), "commentLines": count,
                "iterations": ITERATIONS, "warmup": WARMUP,
                "metadata": metadata,
                "parseAndDropMicroseconds": timings(&parse_samples),
                "rootTraversalMicroseconds": timings(&traversal_samples),
                "grammar": "tree-sitter-python 0.25.0", "runtime": "tree-sitter 0.25.10",
                "parseScope": "reused configured Parser; parse with no old tree; root child count and error queries; Tree destruction; excludes source generation, parser setup, JSON, stdout and process startup",
                "traversalScope": "preparsed tree; root cursor construction; named_children Vec collection and destruction; excludes parsing, Tree destruction, count assertion and output",
            }),
        )?;
        writeln!(stdout)?;
    }
    Ok(())
}
