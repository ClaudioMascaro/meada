import scale, { add as plus, double } from './math';
import * as numbers from './math.js';
import { Circle, Shape } from '@/shapes';
import * as util from './util/index';
import { log, math } from './util';
import { useState } from 'react';

export function main(shape: Shape): void {
  plus(1, 2);
  double(2);
  scale(3, 4);
  numbers.square(2);
  math.add(1, 2);
  util.format(1);
  log('ready');
  Shape.unit();
  Circle.unit();
  shape.describe();
  shape.area();
  legacy();
  render(1);
  useState();
}
