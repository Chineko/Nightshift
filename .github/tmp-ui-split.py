from pathlib import Path

path = Path('crates/nightshift-ui/src/main.rs')
source = path.read_text()

const_start = source.index('const BG:')
bridge_start = source.index('enum UiUpdate')
app_start = source.index('struct NightshiftApp')
main_start = source.index('fn main() -> eframe::Result')

imports = source[:const_start]
constants_and_page = source[const_start:bridge_start]
bridge_body = source[bridge_start:app_start]
app_body = source[app_start:main_start]
main_body = source[main_start:]

bridge_body = bridge_body.replace('enum UiUpdate {', 'pub(crate) enum UiUpdate {', 1)
bridge_body = bridge_body.replace('struct Bridge {', 'pub(crate) struct Bridge {', 1)
bridge_body = bridge_body.replace('    commands: tokio_mpsc::UnboundedSender<UiCommand>,', '    pub(crate) commands: tokio_mpsc::UnboundedSender<UiCommand>,', 1)
bridge_body = bridge_body.replace('    updates: mpsc::Receiver<UiUpdate>,', '    pub(crate) updates: mpsc::Receiver<UiUpdate>,', 1)
bridge_body = bridge_body.replace('    fn spawn(ctx: egui::Context) -> Self {', '    pub(crate) fn spawn(ctx: egui::Context) -> Self {', 1)
bridge_body = bridge_body.replace('    fn send(&self, command: UiCommand) {', '    pub(crate) fn send(&self, command: UiCommand) {', 1)

app_body = app_body.replace('struct NightshiftApp {', 'pub(crate) struct NightshiftApp {', 1)
app_body = app_body.replace('    fn new(ctx: egui::Context) -> Self {', '    pub(crate) fn new(ctx: egui::Context) -> Self {', 1)

root = Path('crates/nightshift-ui/src')
(root / 'bridge.rs').write_text('use super::*;\n\n' + bridge_body.rstrip() + '\n')
(root / 'app.rs').write_text('use super::*;\nuse crate::bridge::Bridge;\n\n' + constants_and_page + app_body.rstrip() + '\n')
path.write_text('mod app;\nmod bridge;\n\n' + imports.rstrip() + '\n\nuse app::NightshiftApp;\n\n' + main_body.rstrip() + '\n')
