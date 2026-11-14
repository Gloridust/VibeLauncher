import { render } from "preact";
import { App } from "./App";
import "./styles.css";

render(<App />, document.getElementById("app")!);

// Click-through test for CI; left out of normal builds.
if (import.meta.env.VITE_SELFTEST === "1") void import("./selftest").then((m) => m.run());
