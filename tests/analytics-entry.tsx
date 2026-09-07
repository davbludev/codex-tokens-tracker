import React, { useState } from "react";
import { createRoot } from "react-dom/client";
import { Projects } from "../src/Projects";
import { Models } from "../src/Models";
import "../src/style.css";
import "../src/pricing.css";

function AnalyticsHarness() {
  const [models, setModels] = useState(false);
  return <main><button type="button" onClick={() => setModels(!models)}>Switch view</button>{models ? <Models /> : <Projects />}</main>;
}
createRoot(document.getElementById("root")!).render(<AnalyticsHarness />);
