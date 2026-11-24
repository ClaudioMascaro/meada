function helper() {}

function createThing(o: object) {
  return o;
}

export const plugin = createThing({
  register() {
    helper();
  },
});
