---
description: 'Start Qt printer workers asynchronously; never run or terminate them directly'
astCondition:
  - '$X._worker.run()'
  - '$X.worker.run()'
  - '$X._worker.terminate()'
  - '$X.worker.terminate()'
scope: 'tool:edit(packages/desktop/fiscal_desktop/app.py), tool:write(packages/desktop/fiscal_desktop/app.py)'
---

Use `start()` for the single `Worker` QThread and update widgets through its status signals. Calling `run()` directly blocks the GUI thread. Stop cooperatively and let the current fiscal operation finish; do not terminate a worker mid-print. Follow `Worker` and `MainWindow.start`/`MainWindow.stop` in `packages/desktop/fiscal_desktop/app.py`; manual reports go through the same worker and printer lock.
