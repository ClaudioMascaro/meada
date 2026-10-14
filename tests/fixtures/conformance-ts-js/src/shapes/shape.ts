export class Shape {
  static unit(): Shape {
    return new Shape();
  }

  area(): number {
    return 0;
  }

  describe(): string {
    return `area ${this.area()}`;
  }
}
