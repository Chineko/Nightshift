from pathlib import Path
import re

path = Path('crates/nightshift-daemon/src/main.rs')
source = path.read_text()

state_start = source.index('fn detector_config_from_state')
ipc_start = source.index('struct OverlayClientGuard')
main_start = source.index('#[tokio::main]')
tests_start = source.index('#[cfg(test)]\nmod tests {')

header = source[:state_start]
state_body = source[state_start:ipc_start]
ipc_body = source[ipc_start:main_start]
main_body = source[main_start:tests_start]
tests_wrapped = source[tests_start:]

header = header.replace('mod config;\n', 'mod config;\nmod ipc;\nmod state;\n')
header += 'use ipc::*;\nuse state::*;\n\n'

state_body = state_body.replace('fn detector_config_from_state', 'pub(crate) fn detector_config_from_state', 1)
state_body = state_body.replace('fn normalize_hr_devices', 'pub(crate) fn normalize_hr_devices', 1)
state_body = state_body.replace('#[derive(Clone)]\nstruct StateStore {', '#[derive(Clone)]\npub(crate) struct StateStore {', 1)

match = re.search(r'pub\(crate\) struct StateStore \{\n(.*?)\n\}\n\nimpl StateStore', state_body, re.S)
if not match:
    raise SystemExit('StateStore struct not found')
fields = []
for line in match.group(1).splitlines():
    if line.startswith('    ') and line.strip():
        line = '    pub(crate) ' + line.strip()
    fields.append(line)
state_body = state_body[:match.start(1)] + '\n'.join(fields) + state_body[match.end(1):]
state_body = re.sub(r'(?m)^    (async )?fn ', lambda m: '    pub(crate) ' + (m.group(1) or '') + 'fn ', state_body)

for signature in [
    'fn socket_path()',
    'async fn send_message(',
    'async fn read_ipc_line<R>(',
    'async fn handle_client(',
]:
    ipc_body = ipc_body.replace(signature, 'pub(crate) ' + signature, 1)

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

Path('crates/nightshift-daemon/src/state.rs').write_text('use super::*;\n\n' + state_body.rstrip() + '\n')
Path('crates/nightshift-daemon/src/ipc.rs').write_text('use super::*;\n\n' + ipc_body.rstrip() + '\n')
Path('crates/nightshift-daemon/src/tests.rs').write_text(tests_body)
path.write_text(header.rstrip() + '\n\n' + main_body.rstrip() + '\n\n#[cfg(test)]\nmod tests;\n')
