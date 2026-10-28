function decl() {}
let letArrow = () => {};
var varFn = function () {};
exports.expA = function () {};
module.exports.expB = function named() {};
function V() {}
V.prototype.proto = function proto() {};
var app = {};
app.use = function use(fn) {};
const o = { litM() {}, litP: function () {} };
var f = function () {};
f = decl;
function caller() { decl(); app.use(); o.litM(); new V().proto(); varFn(); letArrow(); f(); }
function hidden(app, o) { app.use(); o.litM(); }
function innerDef() { const app = () => {}; app.use(); }
function innerCatch() { try {} catch (o) { o.litM(); } }
