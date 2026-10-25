from pathlib import Path
import re

path = Path('crates/nightshift-vrchat-worker/src/main.rs')
source = path.read_text()

runtime_start = source.index('struct AbortTask')
pipeline_start = source.index('fn pipeline_error_message')
session_start = source.index('async fn load_session')
connection_start = source.index('async fn write_client_message')
main_start = source.index('#[tokio::main]')
tests_start = source.index('#[cfg(test)]\nmod tests {')

header = source[:runtime_start]
runtime_body = source[runtime_start:pipeline_start] + '\n' + source[session_start:connection_start]
pipeline_body = source[pipeline_start:session_start]
connection_body = source[connection_start:main_start]
main_body = source[main_start:tests_start]
tests_wrapped = source[tests_start:]

header = 'mod connection;\nmod pipeline;\nmod runtime;\n\n' + header
header += 'use connection::*;\nuse pipeline::*;\nuse runtime::*;\n\n'

runtime_body = runtime_body.replace(
    'struct AbortTask(Option<JoinHandle<()>>);',
    'pub(crate) struct AbortTask(pub(crate) Option<JoinHandle<()>>);',
    1,
)
for name in ['RuntimeRefresh', 'StatusTaskResult']:
    runtime_body = runtime_body.replace(f'struct {name} {{', f'pub(crate) struct {name} {{', 1)
    pattern = rf'pub\(crate\) struct {name} \{{\n(.*?)\n\}}'
    match = re.search(pattern, runtime_body, re.S)
    if not match:
        raise SystemExit(f'{name} struct not found')
    fields = []
    for line in match.group(1).splitlines():
        if line.startswith('    ') and line.strip():
            line = '    pub(crate) ' + line.strip()
        fields.append(line)
    runtime_body = runtime_body[:match.start(1)] + '\n'.join(fields) + runtime_body[match.end(1):]

runtime_body = re.sub(r'(?m)^fn ', 'pub(crate) fn ', runtime_body)
runtime_body = re.sub(r'(?m)^async fn ', 'pub(crate) async fn ', runtime_body)
runtime_body = re.sub(r'(?m)^    fn ', '    pub(crate) fn ', runtime_body)
runtime_body = runtime_body.replace(
    'impl Drop for AbortTask {\n    pub(crate) fn drop(&mut self) {',
    'impl Drop for AbortTask {\n    fn drop(&mut self) {',
    1,
)
pipeline_body = re.sub(r'(?m)^fn ', 'pub(crate) fn ', pipeline_body)
pipeline_body = re.sub(r'(?m)^async fn ', 'pub(crate) async fn ', pipeline_body)
connection_body = re.sub(r'(?m)^fn ', 'pub(crate) fn ', connection_body)
connection_body = re.sub(r'(?m)^async fn ', 'pub(crate) async fn ', connection_body)

prefix = '#[cfg(test)]\nmod tests {'
if not tests_wrapped.startswith(prefix) or not tests_wrapped.rstrip().endswith('}'):
    raise SystemExit('tests wrapper shape changed')
inner = tests_wrapped[len(prefix):].rstrip()
inner = inner[:-1]
if inner.startswith('\n'):
    inner = inner[1:]
test_lines = []
for line in inner.splitlines():
    test_lines.append(line[4:] if line.startswith('    ') else line)
tests_body = '\n'.join(test_lines).rstrip() + '\n'

root = Path('crates/nightshift-vrchat-worker/src')
(root / 'runtime.rs').write_text('use super::*;\n\n' + runtime_body.rstrip() + '\n')
(root / 'pipeline.rs').write_text('use super::*;\n\n' + pipeline_body.rstrip() + '\n')
(root / 'connection.rs').write_text('use super::*;\n\n' + connection_body.rstrip() + '\n')
(root / 'tests.rs').write_text(tests_body)
path.write_text(header.rstrip() + '\n\n' + main_body.rstrip() + '\n\n#[cfg(test)]\nmod tests;\n')
