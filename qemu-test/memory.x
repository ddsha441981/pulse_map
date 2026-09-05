/* Both machines put FLASH at 0 and RAM at 0x20000000, so one script links for
   both. The sizes are the smaller pair — the micro:bit's nRF51822 (256K flash,
   16K RAM); lm3s6965evb has 256K/64K and is happy with less. */
MEMORY
{
  FLASH : ORIGIN = 0x00000000, LENGTH = 256K
  RAM   : ORIGIN = 0x20000000, LENGTH = 16K
}
