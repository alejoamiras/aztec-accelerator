import { invoke, wireButton } from "./bridge.js";

wireButton("visit-presto", {
  disableAlso: "dismiss",
  onClick: async () => {
    await invoke("respond_migration_notice", { openPresto: true });
  },
});

wireButton("dismiss", {
  disableAlso: "visit-presto",
  onClick: async () => {
    await invoke("respond_migration_notice", { openPresto: false });
  },
});
