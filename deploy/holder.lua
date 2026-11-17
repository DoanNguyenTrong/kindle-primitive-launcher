-- Primitive Launcher: holds com.lab126.kaf frameworkStarted = 1 while the stock framework is stopped.
--
-- powerd only answers the power button (and goes to sleep) when something says the UI is running. KOReader
-- sets this property itself when it runs with the framework stopped (koreader frontend, device/kindle
-- frameworkStopped()); this is the same thing, kept alive for as long as the launcher runs. Tested on the
-- Kindle (device-notes.md, S2b): with the holder, button sleep and wake work with the framework stopped.
--
-- Run by deploy/supervisor.sh with the stock Lua (/usr/bin/lua and /usr/lib/lua/liblipclua.so). The
-- supervisor kills this process when the stock UI is given back.
package.cpath = "/usr/lib/lua/?.so;" .. package.cpath
local lipc = require("liblipclua")
local handle = lipc.init("com.lab126.kaf")
if not handle then
    io.stderr:write("holder: could not get a lipc handle\n")
    os.exit(1)
end
local prop = handle:register_int_property("frameworkStarted", "r")
prop.value = 1
io.write("holder: frameworkStarted held\n")
io.flush()
while true do
    os.execute("sleep 30")
end
