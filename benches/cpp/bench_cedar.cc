#include <unistd.h>
#include <fcntl.h>
#include <sys/time.h>
#include <cstdio>
#include <cstring>
#include <cstddef> // for ternary search tree
#include <vector>
#include <string>
#include <cedarpp.h>

// static const
static const size_t BUFFER_SIZE = 1 << 16;

#define KEY_SEP '\n'
inline char *find_sep(char *p)
{
  while (*p != '\n')
    ++p;
  *p = '\0';
  return p;
}

typedef cedar::da<int> cedar_t;

template <typename T>
inline T *create() { return new T(); }

template <typename T>
inline void destroy(T *t) { delete t; }

size_t read_data(const char *file, char *&data)
{
  int fd = ::open(file, O_RDONLY);
  if (fd < 0)
  {
    std::fprintf(stderr, "no such file: %s\n", file);
    std::exit(1);
  }
  size_t size = static_cast<size_t>(::lseek(fd, 0L, SEEK_END));
  data = new char[size];
  ::lseek(fd, 0L, SEEK_SET);
  ::read(fd, data, size);
  ::close(fd);
  return size;
}

void insert_key(cedar_t *t, const char *key, size_t len, int n)
{
  t->update(key, len) = n;
}

bool lookup_key(cedar_t *t, const char *key, size_t len)
{
  return t->exactMatchSearch<int>(key, len) >= 0;
}

template <typename T>
void insert(T *t, int fd, int &n)
{
  char data[BUFFER_SIZE];
  char *start(data), *end(data), *tail(data + BUFFER_SIZE - 1), *tail_(data);
  while ((tail_ = end + ::read(fd, end, tail - end)) != end)
  {
    for (*tail_ = KEY_SEP; (end = find_sep(end)) != tail_; start = ++end)
      insert_key(t, start, end - start, ++n);
    std::memmove(data, start, tail_ - start);
    end = data + (tail_ - start);
    start = data;
  }
}

// lookup
template <typename T>
void lookup(T *t, char *data, size_t size, int &n_, int &n)
{
  for (char *start(data), *end(data), *tail(data + size);
       end != tail; start = ++end)
  {
    end = find_sep(end);
    if (lookup_key(t, start, end - start))
      ++n_;
    ++n;
  }
}

template <typename T>
void bench(const char *keys, const char *queries, const char *label)
{
  std::fprintf(stderr, "---- %-25s --------------------------\n", label);
  T *t = create<T>();
  struct timeval st, et;
  {
    int fd = ::open(keys, O_RDONLY);
    if (fd < 0)
    {
      std::fprintf(stderr, "no such file: %s\n", keys);
      std::exit(1);
    }
    // build trie
    int n = 0;
    ::gettimeofday(&st, NULL);
    insert(t, fd, n);
    ::gettimeofday(&et, NULL);
    double elapsed = (et.tv_sec - st.tv_sec) + (et.tv_usec - st.tv_usec) * 1e-6;
    std::fprintf(stderr, "%-20s %.2f sec (%.2f nsec per key)\n",
                 "Time to insert:", elapsed, elapsed * 1e9 / n);
    std::fprintf(stderr, "%-20s %d\n\n", "Words:", n);
    ::close(fd);
  }
  if (std::strcmp(queries, "-") != 0)
  {
    // load data
    char *data = 0;
    const size_t size = read_data(queries, data);
    // search
    int n(0), n_(0);
    ::gettimeofday(&st, NULL);
    lookup(t, data, size, n_, n);
    ::gettimeofday(&et, NULL);
    double elapsed = (et.tv_sec - st.tv_sec) + (et.tv_usec - st.tv_usec) * 1e-6;
    std::fprintf(stderr, "%-20s %.2f sec (%.2f nsec per key)\n",
                 "Time to search:", elapsed, elapsed * 1e9 / n);
    std::fprintf(stderr, "%-20s %d\n", "Words:", n);
    std::fprintf(stderr, "%-20s %d\n", "Found:", n_);
    delete[] data;
  }
  destroy(t);
}

int main(int argc, char **argv)
{
  if (argc < 3)
  {
    std::fprintf(stderr, "Usage: %s keys queries\n", argv[0]);
    std::exit(1);
  }
  bench<cedar_t>(argv[1], argv[2], "cedar");
}
