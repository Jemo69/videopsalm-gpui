// Stage view window: current + next + clock + alert overlay text + whole-song view (B-20).

interface StageState {
  current: string;
  next: string;
  clock: string;
  alert?: { visible: boolean; text: string };
  wholeSong?: string[];
  activeIndex?: number;
}

let showWholeSong = false;
let lastState: StageState | null = null;

function render(state: StageState): void {
  lastState = state;
  const currentEl = document.getElementById("stage-current");
  if (currentEl) currentEl.textContent = state.current || "(nothing)";
  const nextEl = document.getElementById("stage-next");
  if (nextEl) nextEl.textContent = state.next || "(nothing)";
  const clockEl = document.getElementById("stage-clock");
  if (clockEl) clockEl.textContent = state.clock || "--:--";
  const alertBox = document.getElementById("stage-alert");
  if (alertBox) {
    const visible = !!state.alert?.visible;
    alertBox.hidden = !visible;
    if (visible) alertBox.textContent = `⚠ ${state.alert!.text}`;
  }

  const splitView = document.getElementById("stage-split-view");
  const wholeView = document.getElementById("stage-whole-view");
  const toggleBtn = document.getElementById("stage-toggle-whole");

  if (toggleBtn) {
    toggleBtn.classList.toggle("active", showWholeSong);
  }

  if (splitView && wholeView) {
    if (showWholeSong && state.wholeSong && state.wholeSong.length > 0) {
      splitView.hidden = true;
      wholeView.hidden = false;
      wholeView.innerHTML = "";
      state.wholeSong.forEach((verseText, idx) => {
        const v = document.createElement("div");
        v.className = "whole-song-verse" + (idx === state.activeIndex ? " live" : "");
        v.textContent = verseText;
        wholeView.appendChild(v);
      });
    } else {
      splitView.hidden = false;
      wholeView.hidden = true;
    }
  }
}

document.getElementById("stage-toggle-whole")?.addEventListener("click", () => {
  showWholeSong = !showWholeSong;
  if (lastState) render(lastState);
});

window.vp.onStageRender((data) => render(data as StageState));
void (window.vp.getStageState() as Promise<StageState>).then(render);

export {};
