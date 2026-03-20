/**
 * Insert the minimal DOM fixture needed by ui.ts and main.ts functions.
 * Call in beforeEach and pair with document.body.innerHTML = "" in afterEach.
 */
export function setupDOM(): void {
  document.body.innerHTML = `
    <span id="clock"></span>
    <span id="aztec-status" class="status-dot status-unknown"></span>
    <span id="network-label"></span>
    <span id="aztec-url"></span>
    <span id="accelerator-status" class="status-dot status-unknown"></span>
    <span id="accelerator-label"></span>
    <span id="wallet-dot" class="status-dot status-unknown"></span>
    <span id="wallet-state"></span>
    <div id="versions-row" class="hidden">
      <span id="versions-info"></span>
    </div>
    <button id="mode-local"></button>
    <button id="mode-accelerated"></button>
    <button id="deploy-btn" disabled></button>
    <button id="token-flow-btn" disabled></button>
    <div id="progress" class="hidden">
      <pre id="ascii-art" class="ascii-art"></pre>
    </div>
    <section id="results" class="hidden"></section>
    <div id="result-local"></div>
    <div id="time-local"></div>
    <div id="tag-local"></div>
    <div id="steps-local" class="hidden"></div>
    <div id="result-accelerated"></div>
    <div id="time-accelerated"></div>
    <div id="tag-accelerated"></div>
    <div id="steps-accelerated" class="hidden"></div>
    <div id="log"></div>
    <span id="log-count"></span>
    <button id="export-diagnostics-btn"></button>
    <div id="embedded-ui" class="hidden"></div>
  `;
}
