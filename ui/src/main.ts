import "./lib/theme"; // first, so the page is never painted in the wrong theme
import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

export default mount(App, { target: document.getElementById("app")! });
