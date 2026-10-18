mod health;

use crate::store::Store;

pub fn serve(store: &Store) {
    health::check();
    super::compute();
    store.save();
}
