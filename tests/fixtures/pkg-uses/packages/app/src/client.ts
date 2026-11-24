function makeClient() {
  return {
    get() {
      return 1;
    },
  };
}

export const client = makeClient();
