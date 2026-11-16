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
- Enter Chinese in tab/workspace names, file create/rename/move prompts,
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
