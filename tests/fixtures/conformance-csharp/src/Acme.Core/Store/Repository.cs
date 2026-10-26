namespace Acme.Core
{
    namespace Store
    {
        public class Repository<T>
        {
            public void Save(T item) { }

            public void Save(List<T> items, int batch) { }

            public void SaveAll(params T[] items)
            {
                Save(items[0]);
            }

            public void Flush()
            {
                SaveAll();
                SaveAll(default, default);
            }
        }
    }
}
