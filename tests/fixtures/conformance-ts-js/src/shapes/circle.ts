import { square } from '../math';
import { Shape } from './shape';

export class Circle extends Shape {
  constructor(private r: number) {
    super();
  }

  area(): number {
    return 3 * square(this.r);
  }
}
