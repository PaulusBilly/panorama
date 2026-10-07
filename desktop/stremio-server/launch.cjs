// Entry point for the bundled Stremio streaming server. server.js is shipped
// unmodified; this wrapper narrows what it exposes before loading it.
const net = require("node:net");
const path = require("node:path");

// server.js listens on its service port without naming a host, which binds
// every interface. Panorama only ever reaches it over IPv4 loopback.
const listen = net.Server.prototype.listen;
net.Server.prototype.listen = function (...args) {
  const [port, host] = args;
  if (typeof port === "number" && port >= 11470 && port <= 11474 && typeof host !== "string") {
    return listen.call(this, port, "127.0.0.1", ...args.slice(1));
  }
  return listen.apply(this, args);
};

// A crashed or force-quit Panorama cannot stop its server, so the server
// leaves once the process that started it is gone.
const parent = Number(process.env.PANORAMA_PARENT_PID);
if (Number.isInteger(parent) && parent > 0) {
  setInterval(() => {
    try {
      process.kill(parent, 0);
    } catch (error) {
      if (error.code === "ESRCH") process.exit(0);
    }
  }, 2_000).unref();
}

require(path.join(__dirname, "server.js"));
