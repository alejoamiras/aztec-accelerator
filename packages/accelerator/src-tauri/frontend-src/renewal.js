import { invoke, wireButton } from "./bridge.js";

// F-012: Rust closes the window after each command succeeds (the page has no core:window grant); a
// failure leaves it open with wireButton's error hint.
wireButton("renew", {
  disableAlso: "later",
  loadingText: "Renewing…",
  onClick: async () => {
    await invoke("renew_cert");
  },
});
wireButton("later", {
  onClick: async () => {
    await invoke("record_renewal_prompt");
  },
});
