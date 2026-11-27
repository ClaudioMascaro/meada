class Base { send() {} }
class Sub extends Base { send = () => {}; }
export function run(b: Base) { b.send(); }
