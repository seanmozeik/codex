"""Sequential, inert release measurements; run from codex-rs after building.

Usage: python3 code-activity/examples/measure.py ORIGINAL_COMPARE OUTPUT_DIR
No source program is executed: subprocesses are only the two Rust harnesses.
"""
import gzip
import hashlib
import json
import math
import pathlib
import platform
import statistics
import subprocess
import sys
import tempfile
import time


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    original = str(pathlib.Path(sys.argv[1]).resolve())
    output = pathlib.Path(sys.argv[2]).resolve()
    output.mkdir(parents=True, exist_ok=True)
    benchmark = str(pathlib.Path('target/release/examples/benchmark').resolve())
    compare = str(pathlib.Path('target/release/examples/compare').resolve())
    commands = []

    def run(args, *, memory=False):
        command = ['/usr/bin/nice', '-n', '10', *args]
        if memory:
            command = ['/usr/bin/time', '-l', *command]
        commands.append(command)
        result = subprocess.run(command, capture_output=True, check=True, timeout=300)
        return result.stdout, result.stderr

    corpus, _ = run([benchmark, '--emit-corpus'])
    inputs = [json.loads(line) for line in corpus.splitlines()]
    with (output / 'corpus-v1.jsonl.gz').open('wb') as file:
        # Stable gzip header; expanded bytes are independently hashed below.
        with gzip.GzipFile(filename='', mode='wb', fileobj=file, mtime=0) as archive:
            archive.write(corpus)
    pilot, _ = run([benchmark, '1', '0', 'all', 'consumer'])
    (output / 'pilot-consumer.jsonl').write_bytes(pilot)
    pilots = {row['case']: row for row in map(json.loads, pilot.splitlines())}
    raw = output / 'measurements.jsonl'
    rss_cases = {
        'python-direct-1', 'python-direct-128', 'javascript-direct-128',
        'typescript-functions-128', 'shell-direct-128', 'shell-nested-node-32',
        'typescript-tools-shell-python-32', 'typeScript-malformed-1024',
        'source-byte-boundary-1048576', 'source-byte-boundary-1048577',
        'python-alias-expansion-128', 'javascript-alias-expansion-128',
    }
    started = time.time()
    with raw.open('wb') as file:
        for index, source in enumerate(inputs):
            name = source['callId']
            # Fixed, published pilot thresholds bound measurement effort.
            latency = pilots[name]['microseconds']['mean']
            iterations, warmup = (3, 1) if latency > 50_000 else (
                (10, 2) if latency > 5_000 else (30, 3))
            for mode in ['parse-emit', 'consumer']:
                memory = mode == 'consumer' and name in rss_cases
                stdout, stderr = run(
                    [benchmark, str(iterations), str(warmup), name, mode],
                    memory=memory,
                )
                file.write(stdout)
                file.flush()
                if memory:
                    (output / f'{name}.time.txt').write_bytes(stderr)
            print(f'{index + 1}/{len(inputs)} {name}', flush=True)

    # Same source harness and timed scope in both parsers. Outputs that differ
    # are retained as semantic deltas, never presented as equivalent work.
    families = ['python-direct', 'python-vars-functions', 'python-closure',
                'javascript-direct', 'javascript-closure', 'javascript-callback',
                'typescript-functions', 'javascript-process', 'shell-direct',
                'shell-nested-node', 'shell-nested-python',
                'javascript-awaited-promises', 'python-lazy-generator',
                'javascript-deferred-import', 'python-alias-expansion',
                'javascript-alias-expansion']
    selected = [source for source in inputs if any(
        source['callId'] == f'{family}-{scale}'
        for family in families for scale in [1, 128, 32])]
    with tempfile.TemporaryDirectory(prefix='parser-common-') as directory:
        path = pathlib.Path(directory) / 'common.jsonl'
        path.write_text(''.join(json.dumps(row) + '\n' for row in selected))
        (output / 'common-case-ids.json').write_text(
            json.dumps([row['callId'] for row in selected], indent=2) + '\n')
        reports = {}
        for label, binary in [('original-125f907', original), ('current', compare)]:
            stdout, stderr = run([binary, str(path), '30', '3'], memory=True)
            (output / f'common-{label}.jsonl').write_bytes(stdout)
            (output / f'common-{label}.time.txt').write_bytes(stderr)
            stdout, _ = run([binary, str(path), '1', '0', '--reports'])
            reports[label] = stdout.splitlines()
        equivalence = []
        for source, before, after in zip(selected, reports['original-125f907'],
                                         reports['current'], strict=True):
            equivalence.append({
                'case': source['callId'], 'byteIdentical': before == after,
                'originalReportSha256': hashlib.sha256(before).hexdigest(),
                'currentReportSha256': hashlib.sha256(after).hexdigest(),
                'originalCounts': {key: len(json.loads(before)[key])
                                   for key in ['operations', 'unresolved', 'sources']},
                'currentCounts': {key: len(json.loads(after)[key])
                                  for key in ['operations', 'unresolved', 'sources']},
            })
        (output / 'common-equivalence.json').write_text(json.dumps(equivalence, indent=2) + '\n')

    receipt = {
        'version': 1, 'startedUnix': started, 'finishedUnix': time.time(),
        'platform': platform.platform(), 'machine': platform.machine(),
        'python': sys.version, 'caseCount': len(inputs),
        'corpusExpandedSha256': hashlib.sha256(corpus).hexdigest(),
        'benchmarkBinarySha256': digest(pathlib.Path(benchmark)),
        'compareBinarySha256': digest(pathlib.Path(compare)),
        'originalCompareBinarySha256': digest(pathlib.Path(original)),
        'sampling': 'pilot consumer mean >50ms: 3/1; >5ms: 10/2; otherwise 30/3 iterations/warmup',
        'commands': commands,
    }
    receipt['codeCommit'] = subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip()
    receipt['rustc'] = subprocess.check_output(['rustc', '-Vv'], text=True)
    receipt['cargo'] = subprocess.check_output(['cargo', '-V'], text=True).strip()
    receipt['hardware'] = subprocess.check_output(
        ['/usr/sbin/sysctl', 'hw.model', 'hw.ncpu', 'hw.memsize', 'machdep.cpu.brand_string'], text=True)
    receipt['os'] = subprocess.check_output(['/usr/bin/sw_vers'], text=True)
    receipt['sourceSha256'] = {str(path): digest(path) for path in sorted(
        pathlib.Path('code-activity').rglob('*')) if path.is_file() and (
            path.suffix == '.rs' or path.name in ['benchmark-v1.json', 'measure.py'])}
    (output / 'run-receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')

    rows = [json.loads(line) for line in raw.read_text().splitlines()]
    def summary(samples):
        ordered = sorted(samples)
        return {'p50': ordered[math.ceil(len(samples) * .50) - 1],
                'p95': ordered[math.ceil(len(samples) * .95) - 1],
                'mean': statistics.mean(samples),
                'populationStddev': statistics.pstdev(samples)}
    comparison = []
    before = {row['case']: row for row in map(json.loads, (output / 'common-original-125f907.jsonl').read_text().splitlines())}
    after = {row['case']: row for row in map(json.loads, (output / 'common-current.jsonl').read_text().splitlines())}
    for item in equivalence:
        name = item['case']
        comparison.append({**item, 'originalMicroseconds': summary(before[name]['microsecondsRaw']),
                           'currentMicroseconds': summary(after[name]['microsecondsRaw'])})
    (output / 'comparison-summary.json').write_text(json.dumps(comparison, indent=2) + '\n')
    print(f'Completed {len(rows)} pipeline measurements, {len(comparison)} common comparisons.')


if __name__ == '__main__':
    main()
