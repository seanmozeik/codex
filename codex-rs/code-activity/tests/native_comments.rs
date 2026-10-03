//! Comment-scanner regressions use inert source; no submitted program executes.

use codex_code_activity::ActivityContract;
use codex_code_activity::Analyzer;
use codex_code_activity::Basis;
use codex_code_activity::Coverage;
use codex_code_activity::Effect;
use codex_code_activity::Language;
use codex_code_activity::Report;
use codex_code_activity::Request;
use codex_code_activity::Target;
use codex_code_activity::UiAction;
use codex_code_activity::policy::MockExecutor;
use codex_code_activity::policy::MockPermission;
use codex_code_activity::policy::PolicyDisposition;
use codex_code_activity::policy::PolicyRule;
use codex_code_activity::policy::review_script;
use pretty_assertions::assert_eq;
use tree_sitter::Node;
use tree_sitter::Parser;
use tree_sitter::Tree;

fn analyze(language: Language, source: &str) -> eyre::Result<Report> {
    Ok(Analyzer::new()?.analyze(Request::new(
        "native-comments".into(),
        language,
        source.into(),
        None,
    )))
}

fn python_tree(source: &str) -> eyre::Result<Tree> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    parser
        .parse(source, None)
        .ok_or_else(|| eyre::eyre!("parser interrupted"))
}

fn comment_nodes<'a>(node: Node<'a>, comments: &mut Vec<Node<'a>>) {
    if node.kind() == "comment" {
        comments.push(node);
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        comment_nodes(child, comments);
    }
}

fn read_paths(report: &Report) -> Vec<&str> {
    report
        .operations
        .iter()
        .filter_map(|operation| match &operation.effect {
            Effect::FileRead {
                target: Target::Literal { path, .. },
            } => Some(path.as_str()),
            _ => None,
        })
        .collect()
}

fn assert_read_coordinates(
    report: &Report,
    source: &str,
    expression: &str,
    line: usize,
    column: usize,
) -> eyre::Result<()> {
    let operation = report
        .operations
        .iter()
        .find(|operation| {
            matches!(operation.effect, Effect::FileRead { .. })
                && source.get(operation.evidence.start_byte..operation.evidence.end_byte)
                    == Some(expression)
        })
        .ok_or_else(|| eyre::eyre!("expected exact read-expression evidence"))?;
    let start = source
        .find(expression)
        .ok_or_else(|| eyre::eyre!("expression belongs to fixture"))?;
    assert_eq!(operation.evidence.start_byte, start);
    assert_eq!(operation.evidence.end_byte, start + expression.len());
    assert_eq!(operation.evidence.start_line, line);
    assert_eq!(operation.evidence.end_line, line);
    assert_eq!(operation.evidence.start_column, column);
    assert_eq!(operation.basis, Basis::StaticIntent);
    Ok(())
}

#[test]
fn module_comments_before_between_and_after_reads_preserve_full_tree_and_ui() -> eyre::Result<()> {
    let padding = "# inert open('.env').read() – café\n".repeat(24);
    let source =
        format!("{padding}open('first.txt').read()\n{padding}open('second.txt').read()\n{padding}");
    let tree = python_tree(&source)?;
    assert!(!tree.root_node().has_error());
    let mut comments = Vec::new();
    comment_nodes(tree.root_node(), &mut comments);
    assert_eq!(comments.len(), 72);
    assert!(comments.iter().all(|node| source.get(node.byte_range()) == Some("# inert open('.env').read() – café")));

    let report = analyze(Language::Python, &source)?;
    assert_eq!(read_paths(&report), ["first.txt", "second.txt"]);
    assert_eq!(report.operations.len(), 2);
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    assert_read_coordinates(&report, &source, "open('first.txt').read()", 25, 1)?;
    assert_read_coordinates(&report, &source, "open('second.txt').read()", 50, 1)?;
    let contract = ActivityContract::from_report(&report);
    assert_eq!(
        contract
            .ui_rows()
            .iter()
            .map(|row| row.action)
            .collect::<Vec<_>>(),
        [UiAction::Read, UiAction::Read]
    );
    assert_eq!(
        serde_json::to_value(&contract)?["schema"],
        "codex.codeActivity.v1"
    );
    Ok(())
}

#[test]
fn indented_comments_preserve_invoked_function_effects_and_source_coordinates() -> eyre::Result<()>
{
    let padding = "    # ignored mutation('.env')\n".repeat(16);
    let source = format!("def read():\n{padding}    open('inside.txt').read()\n{padding}read()\n");
    let tree = python_tree(&source)?;
    assert!(!tree.root_node().has_error());
    let mut comments = Vec::new();
    comment_nodes(tree.root_node(), &mut comments);
    assert_eq!(comments.len(), 32);
    let report = analyze(Language::Python, &source)?;
    assert_eq!(read_paths(&report), ["inside.txt"]);
    assert_eq!(report.operations.len(), 1);
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    assert_read_coordinates(&report, &source, "open('inside.txt').read()", 18, 5)?;
    Ok(())
}

#[test]
fn mixed_comment_indentation_keeps_first_comment_dedent_ownership() -> eyre::Result<()> {
    for (comments, expected_parents) in [
        ("# outer-first\n    # inner-second\n", ["module", "module"]),
        ("    # inner-first\n# outer-second\n", ["block", "module"]),
    ] {
        let source = format!(
            "def read():\n    open('inside.txt').read()\n{comments}open('outside.txt').read()\nread()\n"
        );
        let tree = python_tree(&source)?;
        assert!(!tree.root_node().has_error());
        let mut nodes = Vec::new();
        comment_nodes(tree.root_node(), &mut nodes);
        let parents = nodes
            .iter()
            .map(|node| node.parent().expect("comment has a parent").kind())
            .collect::<Vec<_>>();
        assert_eq!(parents, expected_parents, "{source}");
        let report = analyze(Language::Python, &source)?;
        assert_eq!(read_paths(&report), ["outside.txt", "inside.txt"]);
        assert_eq!(report.operations.len(), 2);
        assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
        assert_read_coordinates(&report, &source, "open('inside.txt').read()", 2, 5)?;
        assert_read_coordinates(&report, &source, "open('outside.txt').read()", 5, 1)?;
    }
    Ok(())
}

#[test]
fn tabs_crlf_and_unicode_comments_preserve_byte_columns_and_line_counts() -> eyre::Result<()> {
    let source = "# café 🦀\r\ndef read():\r\n\t# λ\r\n\topen('café.txt').read()\r\n\t# 終\r\nread()\r\n# final\r\n";
    let tree = python_tree(source)?;
    assert!(!tree.root_node().has_error());
    let mut comments = Vec::new();
    comment_nodes(tree.root_node(), &mut comments);
    assert_eq!(comments.len(), 4);
    let report = analyze(Language::Python, source)?;
    assert_eq!(read_paths(&report), ["café.txt"]);
    assert_eq!(report.operations.len(), 1);
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    assert_read_coordinates(&report, source, "open('café.txt').read()", 4, 2)?;
    Ok(())
}

#[test]
fn hashes_in_triple_raw_and_formatted_strings_remain_literal_content() -> eyre::Result<()> {
    let source = "# only real comment\nname='formatted'\nopen('''triple#literal.txt''').read()\nopen(r'raw#literal.txt').read()\nopen(f'{name}#literal.txt').read()\n";
    let tree = python_tree(source)?;
    assert!(!tree.root_node().has_error());
    let mut comments = Vec::new();
    comment_nodes(tree.root_node(), &mut comments);
    assert_eq!(comments.len(), 1);
    let report = analyze(Language::Python, source)?;
    assert_eq!(
        read_paths(&report),
        ["triple#literal.txt", "raw#literal.txt"]
    );
    assert_eq!(report.operations.len(), 3);
    assert_eq!(
        report.operations[2].effect,
        Effect::FileRead {
            target: Target::Unresolved
        }
    );
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    assert_read_coordinates(
        &report,
        source,
        "open('''triple#literal.txt''').read()",
        3,
        1,
    )?;
    assert_read_coordinates(&report, source, "open(r'raw#literal.txt').read()", 4, 1)?;
    assert_read_coordinates(&report, source, "open(f'{name}#literal.txt').read()", 5, 1)?;
    Ok(())
}

#[test]
fn invalid_syntax_after_comment_padding_abstains_for_the_whole_source() -> eyre::Result<()> {
    let source = format!(
        "open('earlier.txt').read()\n{}if (\n",
        "# padding\n".repeat(64)
    );
    let report = analyze(Language::Python, &source)?;
    assert_eq!(report.coverage, Coverage::Opaque);
    assert!(report.operations.is_empty());
    assert_eq!(report.unresolved.len(), 1);
    assert_eq!(
        report.unresolved[0].reason,
        "Incomplete or invalid syntax; no effects inferred"
    );
    assert_eq!(report.unresolved[0].evidence.start_byte, 0);
    assert_eq!(report.unresolved[0].evidence.end_byte, source.len());
    assert_eq!(report.unresolved[0].evidence.start_line, 1);
    Ok(())
}

#[test]
fn late_secret_read_after_comments_blocks_the_entire_mock_dispatch() -> eyre::Result<()> {
    let source = format!(
        "open('notes.txt').read()\n{}open('.env').read()\n",
        "# inert approval hint\n".repeat(64)
    );
    let reviewed = review_script(
        &mut Analyzer::new()?,
        Request::new("late-secret".into(), Language::Python, source.clone(), None),
    );
    assert_eq!(read_paths(reviewed.report()), ["notes.txt", ".env"]);
    assert!(reviewed.report().unresolved.is_empty());
    assert_eq!(reviewed.decision().disposition, PolicyDisposition::Block);
    assert_eq!(reviewed.decision().hits.len(), 1);
    assert_eq!(reviewed.decision().hits[0].rule, PolicyRule::ReadEnv);
    assert_eq!(reviewed.decision().hits[0].operation_id.index(), 1);
    assert_read_coordinates(reviewed.report(), &source, "open('.env').read()", 66, 1)?;
    let mut executor = MockExecutor::default();
    executor.dispatch(&reviewed, MockPermission::Granted);
    assert_eq!(executor.calls(), 0);
    Ok(())
}

#[test]
fn shell_python_comments_keep_original_parent_and_child_span_coordinates() -> eyre::Result<()> {
    let python = format!(
        "{}open('nested.txt').read()\n# child tail\n",
        "# child café\n".repeat(24)
    );
    let source = format!("# outer head\npython3 - <<'PY'\n{python}PY\n# outer tail\n");
    let report = analyze(Language::Shell, &source)?;
    assert_eq!(read_paths(&report), ["nested.txt"]);
    assert_eq!(report.sources.len(), 2);
    assert!(report.unresolved.is_empty(), "{:?}", report.unresolved);
    let read = report
        .operations
        .iter()
        .find(|operation| matches!(operation.effect, Effect::FileRead { .. }))
        .expect("nested read");
    assert_eq!(read.evidence.source_id.index(), 1);
    let parent = report.sources[1]
        .parent
        .as_ref()
        .expect("embedded-source parent");
    assert_eq!(parent.source_id.index(), 0);
    assert_eq!(
        parent.start_byte,
        source.find(&python).expect("original child text")
    );
    assert_eq!(&source[parent.start_byte..parent.end_byte], python);
    assert_eq!(parent.start_line, 3);
    assert_eq!(parent.start_column, 1);
    assert_read_coordinates(&report, &python, "open('nested.txt').read()", 25, 1)?;
    let contract = ActivityContract::from_report(&report);
    let rows = contract.ui_rows();
    assert!(
        rows.iter()
            .any(|row| row.action == UiAction::Read && row.evidence.source_id.index() == 1)
    );
    assert!(rows.iter().all(|row| row.basis == Basis::StaticIntent));
    Ok(())
}
