/* Host-only smoke fixture: validates compiler/process/diagnostic integration. */
static unsigned next_tick(unsigned value) { return (value + 1u) % 1000u; }
int main(void) { return next_tick(999u) == 0u ? 0 : 1; }
