# macOS IME smoke test

Use the macOS Pinyin input source. These checks require a real graphical session;
unit tests do not exercise AppKit or the system candidate window.

- At a shell prompt, compose `nihao` and choose `你好`. No pinyin should reach
  the shell before confirmation; the Chinese text should appear once. Enter
  confirming a candidate must not also run the command.
- Repeat in vim insert mode and inside tmux. Confirm ordinary English, arrow
  keys, Ctrl-C, key repeat, and Option-as-Meta still work.
- Cancel a composition with Escape and with backspace until it is empty.
  Neither cancellation should send text or a deletion to the shell.
- While composing, change panes/tabs or open a dialog. No unfinished text
  should appear in the destination. With broadcast enabled, only confirmed
  text should appear in the other panes.
- Compose near the pane's right/bottom edge, resize, split, and change font
  size. The preedit and candidate window should follow the visible caret.
- Type `阿里云` and mixed ASCII/Chinese, then move the cursor left and right.
  Text must stay aligned with the cell grid, with no growing gap before the
  cursor and no displaced duplicate character under a block cursor.
- Enter Chinese in tab names, file create/rename/move prompts,
  startup commands, the command palette, file finder, navigator, command
  history, and file-tree filtering. Confirm searches/filters refresh.
- In terminal search and copy-mode `/` or `?` search, compose a Chinese word
  from the output. Candidate Enter/Escape should not accept/dismiss the
  search; paste must go to the query, never the shell.
- Replace a selection containing Chinese and emoji with a composition.
  Confirm the replacement and cancel it in separate runs; cancellation on
  focus loss should restore the original text and selection.
- In Codex, type two lines with Shift+Enter between them: no message should
  be sent until plain Enter. Also check Alt+Enter and Ctrl-J, and paste a
  multiline Chinese draft; pasting must not submit it.
- While selecting an IME candidate in Codex, Enter must only confirm the
  candidate, not send a message. Shift+Enter after confirmation inserts a newline.
- Check plain Enter, Ctrl-C, Ctrl-D, Tab, Shift-Tab, and modified F1–F12 in
  a shell, vim, and tmux. Ctrl+Tab must still switch the app's tabs. Legacy
  applications may need a binding for CSI-u modified Enter; no complete Kitty
  keyboard-protocol support is advertised by these changes.
- In a raw input probe, Ctrl+/ must produce `0x1f` (the same as Ctrl+_),
  not `0x7f`. Ctrl+? and Backspace must still produce `0x7f`, and ordinary
  `/` and `?` must remain printable. Repeat in an alternate-screen TUI;
  Ctrl+/ should invoke the application's binding rather than delete text.

## Titles and layout

- With shell title hooks disabled, run `printf '\033]2;中文项目\007'`.
  The tab and its drag preview should show `中文项目`, even with a foreground
  program running. OSC 0 and ST-terminated titles should work too. An empty
  title (`printf '\033]2;\007'`) should fall back to the process name, including
  `zsh` at a prompt. A manually renamed tab should keep its name until cleared.
- In the first workspace, `cd ~/Desktop/omnipty`: its label should be
  `1 - omnipty`. Change directories, switch tabs, and focus a different split;
  the directory must follow that workspace's active tab's focused pane.
  Check background workspaces and drag-reordering too; numbering follows the
  list, not the saved name. Go To, delete confirmation, and startup-command
  editor headings must use the same labels.
- New workspace buttons and the panel's `a` key should create a workspace
  without a name prompt. Workspace rename must be absent from the context
  menu, palette, and `r` binding. Pinned names/layouts on disk must be preserved.
- The terminal should extend to the bottom with no reserved status-bar gap;
  toasts sit 8 px from the bottom. View and the command palette must have no
  status-bar toggle. Old `[status_bar]` settings, including `enabled = true`
  and either position, must not restore the bar or cause a config error.
  Broadcast pane borders and copy-mode labels still work.
- Tabs should have only their titles and close buttons, with no number or
  status dot, including while commands run/fail or background output arrives.
  `tabs.show_numbers = true` and `false` must both load without restoring numbers.
  Check numbered shortcuts (`cmd-1` through `cmd-9`), Ctrl+Tab, dragging,
  double-click rename, close buttons, and the add button. Workspace numbers,
  command history, failure flashes, and desktop notifications should be unchanged.

## Pane focus and full-screen copy mode

- Focus a single pane with the drawer open, switch between split panes, and
  zoom a pane. No ordinary blue focus border should appear, and the content
  must not shift on focus changes. Split dividers, inactive-pane dimming,
  broadcast red borders, failure flashes, and configured SSH-host borders remain.
- In a full-screen TUI (vim, less, or Codex's alternate-screen mode), press
  `cmd-shift-v` or `ctrl-w [` to enter OmniPTY copy mode. Check `hjkl`, `v`/`V`,
  `y`/`yy`, and `/`/`?` searches including Chinese. A yank or `q` should return
  input to the TUI; Escape first clears a selection, then leaves copy mode.
- With mouse reporting enabled in the TUI, clicks and drags should select text
  locally in copy mode, and wheel/trackpad scrolling must not send mouse reports
  or arrow keys to the TUI. Keyboard input and paste must not reach it either.
  Mouse/scroll reporting should resume after leaving copy mode.
- Repeat with the macOS window in native full screen and with a zoomed pane.
  The alternate screen has only its current grid, not the shell's scrollback;
  application redraws can still change that grid while copy mode is active.
