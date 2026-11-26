const other = makeThing();
other.add(import('@scope/plugin-a'));
other.inner.add(import('@scope/plugin-a'));
makeThing().add(import('@scope/plugin-a'));
