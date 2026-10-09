// Projector window: resolution-independent fullscreen render of the current
// worship slide + alert overlay (bottom) + clock overlay (top-right).
// Slide HTML arrives pre-styled from the control window (header/body/footer
// templates, bilingual stacking, chord spans, background color/image/video-loop).

function render(state: { slide: { title: string; html: string }; alert: { visible: boolean; text: string } }): void {
  const slide = document.getElementById("slide")!;
  slide.innerHTML = state.slide.html || `<h1>VideoPsalm</h1><p>Nothing projected.</p>`;
  // Full-bleed worship slides carry their own background: drop the vmin padding
  // so color/image/video covers the whole screen; plain slides keep padding.
  slide.classList.toggle("bleed", slide.querySelector(".vp-slide") !== null);
  // Background color carried on the wrapper: mirror it onto the stage so
  // letterboxing never shows a mismatched black bar.
  const wrap = slide.querySelector(".vp-slide") as HTMLElement | null;
  if (wrap) {
    const bg = wrap.style.backgroundColor;
    if (bg) (slide as HTMLElement).style.background = bg;
    else (slide as HTMLElement).style.background = "#000";
  } else {
    (slide as HTMLElement).style.background = "#000";
  }
  // Ensure looping background videos actually play (autoplay attribute + nudge).
  slide.querySelectorAll("video[autoplay]").forEach((v) => {
    const vid = v as HTMLVideoElement;
    void vid.play().catch(() => undefined);
  });
  const overlay = document.getElementById("alert-overlay")!;
  overlay.hidden = !state.alert.visible;
  overlay.textContent = state.alert.text;
}

window.vp.onRendered((data) => render(data as { slide: { title: string; html: string }; alert: { visible: boolean; text: string } }));
window.vp.onClock((text) => {
  document.getElementById("clock-overlay")!.textContent = text;
});

// Initial state (in case projector opened after a slide was projected).
void (window.vp.getProjectorState() as Promise<{ slide: { title: string; html: string }; alert: { visible: boolean; text: string } }>).then(render);

export {};
