
#include <cstdio>
int main () {
  FILE * file = stdout;
  if (!file) return 1;
  if (putc_unlocked (42, file) != 42) return 1;
  if (fclose (file)) return 1;
  file = fopen (path, "r");
  if (!file) return 1;
  if (getc_unlocked (file) != 42) return 1;
  if (fclose (file)) return 1;
  return 0;
}
