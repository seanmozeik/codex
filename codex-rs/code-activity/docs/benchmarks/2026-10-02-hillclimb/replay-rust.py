"""Matched release consumer timings and exact report regression checks.

Usage from codex-rs: python3 code-activity/docs/benchmarks/2026-10-02-hillclimb/replay-rust.py BEFORE_BENCHMARK BEFORE_CLI OUTPUT
Before binaries must be preserved published e20092b releases, or supplied with
--rebuilt-before CHECKOUT whose clean HEAD is that exact commit.
This runs analyzer harnesses on inert input, never source programs.
"""

import gzip
import hashlib
import json
import pathlib
import subprocess
import sys


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    before_benchmark, before_cli, output = map(lambda s: pathlib.Path(s).resolve(), sys.argv[1:4])
    output.mkdir(parents=True, exist_ok=True)
    after_benchmark = pathlib.Path('target/release/examples/benchmark').resolve()
    after_cli = pathlib.Path('target/release/codex-code-activity').resolve()
    frozen = pathlib.Path('code-activity/docs/benchmarks/2026-10-02-current/corpus-v1.jsonl.gz')
    corpus = gzip.decompress(frozen.read_bytes())
    exported = subprocess.check_output(['/usr/bin/nice', '-n', '10', str(after_benchmark), '--emit-corpus'])
    assert corpus == exported, 'expanded corpus changed'
    prior = json.loads(frozen.with_name('run-receipt.json').read_text())
    matches_published = sha(before_benchmark.read_bytes()) == prior['benchmarkBinarySha256']
    rebuilt_commit = None
    if len(sys.argv) > 4:
        assert len(sys.argv) == 6 and sys.argv[4] == '--rebuilt-before'
        checkout = pathlib.Path(sys.argv[5]).resolve()
        rebuilt_commit = subprocess.check_output(['git', '-C', str(checkout), 'rev-parse', 'HEAD'], text=True).strip()
        assert rebuilt_commit == 'e20092b830d80acb90d637e4480f723450eef042'
        subprocess.run(['git', '-C', str(checkout), 'diff', '--quiet', 'HEAD'], check=True)
    assert matches_published or rebuilt_commit is not None, 'before binary changed; use --rebuilt-before with clean pinned checkout'
    inputs = [json.loads(line) for line in corpus.splitlines()]
    commands, rows = [], []
    with (output / 'matched-consumer.jsonl').open('wb') as file:
        for case in ['python-padding-1024', 'python-padding-8192', 'python-direct-1',
                     'python-direct-128', 'javascript-direct-128', 'typescript-functions-128',
                     'shell-direct-128', 'shell-nested-node-32']:
            count, warmup = (3, 1) if 'padding' in case else (30, 3)
            pair = []
            for label, binary in [('before', before_benchmark), ('after', after_benchmark)]:
                command = ['/usr/bin/time', '-l', '/usr/bin/nice', '-n', '10',
                           str(binary), str(count), str(warmup), case, 'consumer']
                result = subprocess.run(command, capture_output=True, check=True, timeout=120)
                commands.append(command)
                row = {**json.loads(result.stdout), 'variant': label}
                file.write((json.dumps(row) + '\n').encode());file.flush()
                (output / f'{case}-{label}.time.txt').write_bytes(result.stderr)
                pair.append(row)
            assert pair[0]['output'] == pair[1]['output'], case
            rows.extend(pair)
            print(case, 'matched', flush=True)
    outputs = {}
    for label, binary in [('before', before_cli), ('after', after_cli)]:
        command = ['/usr/bin/nice', '-n', '10', str(binary)]
        commands.append(command)
        result = subprocess.run(command, input=corpus, capture_output=True, check=True, timeout=120)
        outputs[label] = result.stdout.splitlines()
        with (output / f'reports-{label}.jsonl.gz').open('wb') as file:
            with gzip.GzipFile(filename='', mode='wb', fileobj=file, mtime=0) as archive:
                archive.write(result.stdout)
    assert len(outputs['before']) == len(outputs['after']) == len(inputs) == 111
    comparisons = []
    for source, before, after in zip(inputs, outputs['before'], outputs['after'], strict=True):
        assert before == after, source['callId']
        report = json.loads(after)
        comparisons.append({'case': source['callId'], 'byteIdentical': True,
                            'beforeReportSha256': sha(before), 'afterReportSha256': sha(after),
                            'counts': {key: len(report[key]) for key in ['operations', 'unresolved', 'sources']}})
    (output / 'report-equivalence.json').write_text(json.dumps(comparisons, indent=2) + '\n')
    receipt = {'beforeCompiledCodeCommit': prior['codeCommit'],
               'beforeBinaryMatchesPublishedRun': matches_published,
               'rebuiltBeforeCheckoutCommit': rebuilt_commit,
               'driverSha256': sha(pathlib.Path(__file__).read_bytes()),
               'beforeCodeCommit': 'e20092b830d80acb90d637e4480f723450eef042',
               'afterCodeCommit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
               'corpusExpandedSha256': sha(corpus), 'caseCount': len(inputs),
               'allReportsByteIdentical': True, 'timingRuns': len(rows),
               'commands': commands,
               'binarySha256': {str(path): sha(path.read_bytes()) for path in
                                [before_benchmark, before_cli, after_benchmark, after_cli]},
               'dependencySourceSha256': {str(path): sha(path.read_bytes()) for path in
                                         [pathlib.Path('Cargo.toml'), pathlib.Path('Cargo.lock'),
                                          pathlib.Path('../MODULE.bazel.lock'),
                                          *sorted(pathlib.Path('../third_party/tree-sitter-python').rglob('*'))]
                                         if path.is_file()},
               'sampling': 'padding: 3 measured/1 warmup each; remaining cases:30/3 each; serial nice10, no profiling; time-l RSS includes startup/dataset/allocator retention'}
    (output / 'rust-build-and-replay-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print('PASS:111 exact reports and16 matched consumer runs')


if __name__ == '__main__':
    main()
