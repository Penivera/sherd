// PLACEHOLDER. "Running" a task here just simulates one locally with a
// timer; no command is actually dispatched anywhere. Future: submit to the
// P2P/compute layer and stream back real status updates.

const SAMPLE_COMMANDS = [
  "cargo build --release",
  "pytest -q",
  "ffmpeg -i in.mp4 out.mp4",
  "make all",
];

export function simulateTask(onUpdate) {
  const cmd = SAMPLE_COMMANDS[Math.floor(Math.random() * SAMPLE_COMMANDS.length)];
  const node = `Node ${Math.floor(Math.random() * 5) + 1}`;
  const id = Date.now();
  const task = { id, cmd, node, status: "running" };

  onUpdate({ ...task });
  setTimeout(() => {
    onUpdate({ ...task, status: "done" });
  }, 1400);

  return task;
}
