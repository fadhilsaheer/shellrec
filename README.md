# SHELLREC

Record terminal commands, export clean docs, and replay workflows.

Local-first. Offline-only.

---

## Features

* Record terminal sessions
* Export commands as Markdown or text
* Remove output/timestamps before export
* Label and save recordings
* Replay recorded commands later

---

## Usage

Start recording:

```bash
shellrec start
```

Stop recording:

```bash
shellrec stop
```

Export commands:

```bash
shellrec export session-1 --commands-only
```

Replay a session:

```bash
shellrec replay session-1
```

Add labels:

```bash
shellrec label session-1 docker
```

List recordings:

```bash
shellrec list
```

---

## Example Export

````md
# React Setup

```bash
npm create vite@latest
cd app
npm install
npm run dev
```
````

---

## License

MIT

