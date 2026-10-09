// Stage view window: current + next + clock + alert overlay text.

interface StageState {
  current: string;
  next: string;
  clock: string;
  alert?: { visible: boolean; text: string };
}

function render(state: StageState): void {
  document.getElementById("stage-current")!.textContent = state.current || "(nothing)";
  document.getElementById("stage-next")!.textContent = state.next || "(nothing)";
  document.getElementById("stage-clock")!.textContent = state.clock || "--:--";
  const alertBox = document.getElementById("stage-alert")!;
  const visible = !!state.alert?.visible;
  alertBox.hidden = !visible;
  if (visible) alertBox.textContent = `⚠ ${state.alert!.text}`;
}

window.vp.onStageRender((data) => render(data as StageState));
void (window.vp.getStageState() as Promise<StageState>).then(render);

export {};
