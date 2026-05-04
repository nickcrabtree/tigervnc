# Agent Instructions

## VNC Viewer Testing Guardrail

Do not launch GUI VNC viewer windows on Nick's Mac during automated testing,
trace capture, or ARO-driven investigation unless Nick explicitly asks for a
local interactive viewer.

Use quartz and headless VNC server/test harness workflows for
Rust TigerVNC viewer testing. Follow the existing tests for how to exercise
protocol/cache behaviour without opening a macOS viewer window.
