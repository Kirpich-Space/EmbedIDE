/** Flight / industrial Cortex-M board catalog. No Arduino / ESP / AVR / Pico hobby boards. */

function b(partial) {
  return {
    fpu: null,
    floatAbi: 'soft',
    flashOrigin: '0x08000000',
    ramOrigin: '0x20000000',
    defaultAdapter: 'stlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC'],
    ...partial,
  }
}

const M4HF = { cpu: 'cortex-m4', fpu: 'fpv4-sp-d16', floatAbi: 'hard', rustTarget: 'thumbv7em-none-eabihf' }
const M7HF = { cpu: 'cortex-m7', fpu: 'fpv5-d16', floatAbi: 'hard', rustTarget: 'thumbv7em-none-eabihf' }
const M7SP = { cpu: 'cortex-m7', fpu: 'fpv5-sp-d16', floatAbi: 'hard', rustTarget: 'thumbv7em-none-eabihf' }
const M33HF = { cpu: 'cortex-m33', fpu: 'fpv5-sp-d16', floatAbi: 'hard', rustTarget: 'thumbv8m.main-none-eabihf' }
const M33 = { cpu: 'cortex-m33', fpu: null, floatAbi: 'soft', rustTarget: 'thumbv8m.main-none-eabi' }
const M3 = { cpu: 'cortex-m3', fpu: null, floatAbi: 'soft', rustTarget: 'thumbv7m-none-eabi' }
const M0 = { cpu: 'cortex-m0', fpu: null, floatAbi: 'soft', rustTarget: 'thumbv6m-none-eabi' }
const M0P = { cpu: 'cortex-m0plus', fpu: null, floatAbi: 'soft', rustTarget: 'thumbv6m-none-eabi' }

const BOARDS = [
  // ── STM32F0 ──────────────────────────────────────────────
  b({ id: 'stm32f030r8', name: 'STM32F030R8', family: 'STM32F0', mcu: 'STM32F030R8', ...M0, flashKb: 64, ramKb: 8, cDefine: 'STM32F030x8', openocdTarget: 'stm32f0x' }),
  b({ id: 'stm32f072rb', name: 'STM32F072RB', family: 'STM32F0', mcu: 'STM32F072RB', ...M0, flashKb: 128, ramKb: 16, cDefine: 'STM32F072xB', openocdTarget: 'stm32f0x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'CAN'] }),
  b({ id: 'stm32f091rc', name: 'STM32F091RC', family: 'STM32F0', mcu: 'STM32F091RC', ...M0, flashKb: 256, ramKb: 32, cDefine: 'STM32F091xC', openocdTarget: 'stm32f0x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN'] }),

  // ── STM32F1 ──────────────────────────────────────────────
  b({ id: 'stm32f103c8', name: 'STM32F103C8', family: 'STM32F1', mcu: 'STM32F103C8', ...M3, flashKb: 64, ramKb: 20, cDefine: 'STM32F103xB', openocdTarget: 'stm32f1x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'CAN'] }),
  b({ id: 'stm32f103re', name: 'STM32F103RE', family: 'STM32F1', mcu: 'STM32F103RE', ...M3, flashKb: 512, ramKb: 64, cDefine: 'STM32F103xE', openocdTarget: 'stm32f1x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'CAN'] }),
  b({ id: 'stm32f107vc', name: 'STM32F107VC', family: 'STM32F1', mcu: 'STM32F107VC', ...M3, flashKb: 256, ramKb: 64, cDefine: 'STM32F107xC', openocdTarget: 'stm32f1x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'CAN', 'ETH'] }),

  // ── STM32F3 ──────────────────────────────────────────────
  b({ id: 'stm32f303cc', name: 'STM32F303CC', family: 'STM32F3', mcu: 'STM32F303CC', ...M4HF, flashKb: 256, ramKb: 40, cDefine: 'STM32F303xC', openocdTarget: 'stm32f3x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'CAN', 'USB'] }),
  b({ id: 'stm32f303re', name: 'STM32F303RE', family: 'STM32F3', mcu: 'STM32F303RE', ...M4HF, flashKb: 512, ramKb: 64, cDefine: 'STM32F303xE', openocdTarget: 'stm32f3x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'CAN', 'USB'] }),
  b({ id: 'stm32f334r8', name: 'STM32F334R8', family: 'STM32F3', mcu: 'STM32F334R8', ...M4HF, flashKb: 64, ramKb: 12, cDefine: 'STM32F334x8', openocdTarget: 'stm32f3x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'HRTIM'] }),

  // ── STM32F4 ──────────────────────────────────────────────
  b({ id: 'stm32f401re', name: 'STM32F401RE', family: 'STM32F4', mcu: 'STM32F401RE', ...M4HF, flashKb: 512, ramKb: 96, cDefine: 'STM32F401xE', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB'] }),
  b({ id: 'stm32f405rg', name: 'STM32F405RG', family: 'STM32F4', mcu: 'STM32F405RG', ...M4HF, flashKb: 1024, ramKb: 192, cDefine: 'STM32F405xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'USB'] }),
  b({ id: 'stm32f407vg', name: 'STM32F407VG', family: 'STM32F4', mcu: 'STM32F407VG', ...M4HF, flashKb: 1024, ramKb: 128, cDefine: 'STM32F407xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB'] }),
  b({ id: 'stm32f407zg', name: 'STM32F407ZG', family: 'STM32F4', mcu: 'STM32F407ZG', ...M4HF, flashKb: 1024, ramKb: 192, cDefine: 'STM32F407xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB'] }),
  b({ id: 'stm32f411ce', name: 'STM32F411CE', family: 'STM32F4', mcu: 'STM32F411CE', ...M4HF, flashKb: 512, ramKb: 128, cDefine: 'STM32F411xE', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB'] }),
  b({ id: 'stm32f412zg', name: 'STM32F412ZG', family: 'STM32F4', mcu: 'STM32F412ZG', ...M4HF, flashKb: 1024, ramKb: 256, cDefine: 'STM32F412Zx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'USB'] }),
  b({ id: 'stm32f427vi', name: 'STM32F427VI', family: 'STM32F4', mcu: 'STM32F427VI', ...M4HF, flashKb: 2048, ramKb: 256, cDefine: 'STM32F427xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB'] }),
  b({ id: 'stm32f429zi', name: 'STM32F429ZI', family: 'STM32F4', mcu: 'STM32F429ZI', ...M4HF, flashKb: 2048, ramKb: 256, cDefine: 'STM32F429xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'LTDC', 'USB'] }),
  b({ id: 'stm32f437zi', name: 'STM32F437ZI', family: 'STM32F4', mcu: 'STM32F437ZI', ...M4HF, flashKb: 2048, ramKb: 256, cDefine: 'STM32F437xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'CRYP', 'USB'] }),
  b({ id: 'stm32f446re', name: 'STM32F446RE', family: 'STM32F4', mcu: 'STM32F446RE', ...M4HF, flashKb: 512, ramKb: 128, cDefine: 'STM32F446xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'USB'] }),
  b({ id: 'stm32f469ni', name: 'STM32F469NI', family: 'STM32F4', mcu: 'STM32F469NI', ...M4HF, flashKb: 2048, ramKb: 384, cDefine: 'STM32F469xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'LTDC', 'USB'] }),
  b({ id: 'stm32f479zi', name: 'STM32F479ZI', family: 'STM32F4', mcu: 'STM32F479ZI', ...M4HF, flashKb: 2048, ramKb: 384, cDefine: 'STM32F479xx', openocdTarget: 'stm32f4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'CRYP', 'USB'] }),

  // ── STM32F7 ──────────────────────────────────────────────
  b({ id: 'stm32f722ze', name: 'STM32F722ZE', family: 'STM32F7', mcu: 'STM32F722ZE', ...M7SP, flashKb: 512, ramKb: 256, cDefine: 'STM32F722xx', openocdTarget: 'stm32f7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'USB'] }),
  b({ id: 'stm32f746ng', name: 'STM32F746NG', family: 'STM32F7', mcu: 'STM32F746NG', ...M7SP, flashKb: 1024, ramKb: 320, cDefine: 'STM32F746xx', openocdTarget: 'stm32f7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'LTDC', 'USB'] }),
  b({ id: 'stm32f756zg', name: 'STM32F756ZG', family: 'STM32F7', mcu: 'STM32F756ZG', ...M7SP, flashKb: 1024, ramKb: 320, cDefine: 'STM32F756xx', openocdTarget: 'stm32f7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'CRYP', 'USB'] }),
  b({ id: 'stm32f767zi', name: 'STM32F767ZI', family: 'STM32F7', mcu: 'STM32F767ZI', ...M7HF, flashKb: 2048, ramKb: 512, cDefine: 'STM32F767xx', openocdTarget: 'stm32f7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'LTDC', 'USB'] }),
  b({ id: 'stm32f777zi', name: 'STM32F777ZI', family: 'STM32F7', mcu: 'STM32F777ZI', ...M7HF, flashKb: 2048, ramKb: 512, cDefine: 'STM32F777xx', openocdTarget: 'stm32f7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'CRYP', 'USB'] }),
  b({ id: 'stm32f769ni', name: 'STM32F769NI', family: 'STM32F7', mcu: 'STM32F769NI', ...M7HF, flashKb: 2048, ramKb: 512, cDefine: 'STM32F769xx', openocdTarget: 'stm32f7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'LTDC', 'DSI', 'USB'] }),

  // ── STM32H5 ──────────────────────────────────────────────
  b({ id: 'stm32h563zi', name: 'STM32H563ZI', family: 'STM32H5', mcu: 'STM32H563ZI', ...M33HF, flashKb: 2048, ramKb: 640, cDefine: 'STM32H563xx', openocdTarget: 'stm32h5x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB', 'TRUSTZONE'] }),
  b({ id: 'stm32h573zi', name: 'STM32H573ZI', family: 'STM32H5', mcu: 'STM32H573ZI', ...M33HF, flashKb: 2048, ramKb: 640, cDefine: 'STM32H573xx', openocdTarget: 'stm32h5x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'CRYP', 'USB', 'TRUSTZONE'] }),

  // ── STM32H7 ──────────────────────────────────────────────
  b({ id: 'stm32h723zg', name: 'STM32H723ZG', family: 'STM32H7', mcu: 'STM32H723ZG', ...M7HF, flashKb: 1024, ramKb: 564, cDefine: 'STM32H723xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'ETH', 'USB'] }),
  b({ id: 'stm32h743zi', name: 'STM32H743ZI', family: 'STM32H7', mcu: 'STM32H743ZI', ...M7HF, flashKb: 2048, ramKb: 1024, cDefine: 'STM32H743xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'ETH', 'USB'] }),
  b({ id: 'stm32h745zi', name: 'STM32H745ZI', family: 'STM32H7', mcu: 'STM32H745ZI', ...M7HF, flashKb: 2048, ramKb: 1024, cDefine: 'STM32H745xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'ETH', 'USB', 'DUAL_CORE'] }),
  b({ id: 'stm32h747xi', name: 'STM32H747XI', family: 'STM32H7', mcu: 'STM32H747XI', ...M7HF, flashKb: 2048, ramKb: 1024, cDefine: 'STM32H747xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'ETH', 'LTDC', 'USB', 'DUAL_CORE'] }),
  b({ id: 'stm32h750vb', name: 'STM32H750VB', family: 'STM32H7', mcu: 'STM32H750VB', ...M7HF, flashKb: 128, ramKb: 1024, cDefine: 'STM32H750xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'ETH', 'USB'] }),
  b({ id: 'stm32h753zi', name: 'STM32H753ZI', family: 'STM32H7', mcu: 'STM32H753ZI', ...M7HF, flashKb: 2048, ramKb: 1024, cDefine: 'STM32H753xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'ETH', 'CRYP', 'USB'] }),
  b({ id: 'stm32h7a3zi', name: 'STM32H7A3ZI', family: 'STM32H7', mcu: 'STM32H7A3ZI', ...M7HF, flashKb: 2048, ramKb: 1376, cDefine: 'STM32H7A3xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'USB', 'OCTOSPI'] }),
  b({ id: 'stm32h7b3li', name: 'STM32H7B3LI', family: 'STM32H7', mcu: 'STM32H7B3LI', ...M7HF, flashKb: 2048, ramKb: 1376, cDefine: 'STM32H7B3xx', openocdTarget: 'stm32h7x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'LTDC', 'CRYP', 'USB'] }),

  // ── STM32G0 / G4 ─────────────────────────────────────────
  b({ id: 'stm32g071rb', name: 'STM32G071RB', family: 'STM32G0', mcu: 'STM32G071RB', ...M0P, flashKb: 128, ramKb: 36, cDefine: 'STM32G071xx', openocdTarget: 'stm32g0x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'USB'] }),
  b({ id: 'stm32g0b1re', name: 'STM32G0B1RE', family: 'STM32G0', mcu: 'STM32G0B1RE', ...M0P, flashKb: 512, ramKb: 144, cDefine: 'STM32G0B1xx', openocdTarget: 'stm32g0x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'FDCAN', 'USB'] }),
  b({ id: 'stm32g431cb', name: 'STM32G431CB', family: 'STM32G4', mcu: 'STM32G431CB', ...M4HF, flashKb: 128, ramKb: 32, cDefine: 'STM32G431xx', openocdTarget: 'stm32g4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'FDCAN', 'USB'] }),
  b({ id: 'stm32g474re', name: 'STM32G474RE', family: 'STM32G4', mcu: 'STM32G474RE', ...M4HF, flashKb: 512, ramKb: 128, cDefine: 'STM32G474xx', openocdTarget: 'stm32g4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'FDCAN', 'HRTIM', 'USB'] }),
  b({ id: 'stm32g491re', name: 'STM32G491RE', family: 'STM32G4', mcu: 'STM32G491RE', ...M4HF, flashKb: 512, ramKb: 112, cDefine: 'STM32G491xx', openocdTarget: 'stm32g4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'FDCAN', 'USB'] }),

  // ── STM32L4 / L5 / U5 ────────────────────────────────────
  b({ id: 'stm32l432kc', name: 'STM32L432KC', family: 'STM32L4', mcu: 'STM32L432KC', ...M4HF, flashKb: 256, ramKb: 64, cDefine: 'STM32L432xx', openocdTarget: 'stm32l4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB'] }),
  b({ id: 'stm32l476rg', name: 'STM32L476RG', family: 'STM32L4', mcu: 'STM32L476RG', ...M4HF, flashKb: 1024, ramKb: 128, cDefine: 'STM32L476xx', openocdTarget: 'stm32l4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'USB', 'LCD'] }),
  b({ id: 'stm32l496zg', name: 'STM32L496ZG', family: 'STM32L4', mcu: 'STM32L496ZG', ...M4HF, flashKb: 1024, ramKb: 320, cDefine: 'STM32L496xx', openocdTarget: 'stm32l4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'USB', 'DFSDM'] }),
  b({ id: 'stm32l4r5zi', name: 'STM32L4R5ZI', family: 'STM32L4', mcu: 'STM32L4R5ZI', ...M4HF, flashKb: 2048, ramKb: 640, cDefine: 'STM32L4R5xx', openocdTarget: 'stm32l4x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'OCTOSPI', 'DFSDM'] }),
  b({ id: 'stm32l552ze', name: 'STM32L552ZE', family: 'STM32L5', mcu: 'STM32L552ZE', ...M33HF, flashKb: 512, ramKb: 256, cDefine: 'STM32L552xx', openocdTarget: 'stm32l5x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'TRUSTZONE'] }),
  b({ id: 'stm32u575zi', name: 'STM32U575ZI', family: 'STM32U5', mcu: 'STM32U575ZI', ...M33HF, flashKb: 2048, ramKb: 768, cDefine: 'STM32U575xx', openocdTarget: 'stm32u5x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'OCTOSPI', 'TRUSTZONE'] }),
  b({ id: 'stm32u5a5zj', name: 'STM32U5A5ZJ', family: 'STM32U5', mcu: 'STM32U5A5ZJ', ...M33HF, flashKb: 4096, ramKb: 2512, cDefine: 'STM32U5A5xx', openocdTarget: 'stm32u5x', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'OCTOSPI', 'GFXMMU', 'TRUSTZONE'] }),

  // ── STM32WB / WL (wireless industrial / telemetry) ───────
  b({ id: 'stm32wb55rg', name: 'STM32WB55RG', family: 'STM32WB', mcu: 'STM32WB55RG', ...M4HF, flashKb: 1024, ramKb: 256, cDefine: 'STM32WB55xx', openocdTarget: 'stm32wbx', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'BLE', '802.15.4'] }),
  b({ id: 'stm32wle5jc', name: 'STM32WLE5JC', family: 'STM32WL', mcu: 'STM32WLE5JC', ...M4HF, flashKb: 256, ramKb: 64, cDefine: 'STM32WLE5xx', openocdTarget: 'stm32wlx', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'SUBGHZ', 'LORA'] }),
  b({ id: 'stm32wl55jc', name: 'STM32WL55JC', family: 'STM32WL', mcu: 'STM32WL55JC', ...M4HF, flashKb: 256, ramKb: 64, cDefine: 'STM32WL55xx', openocdTarget: 'stm32wlx', peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'SUBGHZ', 'LORA', 'DUAL_CORE'] }),

  // ── STM32C0 ──────────────────────────────────────────────
  b({ id: 'stm32c031c6', name: 'STM32C031C6', family: 'STM32C0', mcu: 'STM32C031C6', ...M0P, flashKb: 32, ramKb: 12, cDefine: 'STM32C031xx', openocdTarget: 'stm32c0x' }),

  // ── NXP LPC / i.MX RT (industrial / flight computers) ────
  b({
    id: 'lpc55s69', name: 'LPC55S69', family: 'NXP LPC55', mcu: 'LPC55S69', ...M33HF,
    flashKb: 640, ramKb: 320, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'CPU_LPC55S69JBD100', openocdTarget: 'lpc55sx', defaultAdapter: 'cmsis-dap',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'CAN', 'TRUSTZONE'],
  }),
  b({
    id: 'lpc54628', name: 'LPC54628', family: 'NXP LPC54', mcu: 'LPC54628',
    cpu: 'cortex-m4', fpu: 'fpv4-sp-d16', floatAbi: 'hard', rustTarget: 'thumbv7em-none-eabihf',
    flashKb: 512, ramKb: 200, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'CPU_LPC54628JBD208', openocdTarget: 'lpc546xx', defaultAdapter: 'cmsis-dap',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'ETH', 'USB', 'CAN', 'LCD'],
  }),
  b({
    id: 'mimxrt1062', name: 'i.MX RT1062', family: 'NXP i.MX RT', mcu: 'MIMXRT1062DVJ6A', ...M7HF,
    flashKb: 8192, ramKb: 1024, flashOrigin: '0x60000000', ramOrigin: '0x20000000',
    cDefine: 'CPU_MIMXRT1062DVJ6A', openocdTarget: 'imxrt1060', defaultAdapter: 'cmsis-dap',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB', 'FLEXSPI'],
  }),
  b({
    id: 'mimxrt1176', name: 'i.MX RT1176', family: 'NXP i.MX RT', mcu: 'MIMXRT1176DVMAA', ...M7HF,
    flashKb: 16384, ramKb: 2048, flashOrigin: '0x30000000', ramOrigin: '0x20000000',
    cDefine: 'CPU_MIMXRT1176DVMAA', openocdTarget: 'imxrt1170', defaultAdapter: 'cmsis-dap',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB', 'GPU2D', 'DUAL_CORE'],
  }),

  // ── Nordic nRF (avionics telemetry / sensors — not Arduino) ─
  b({
    id: 'nrf52832', name: 'nRF52832', family: 'Nordic nRF52', mcu: 'nRF52832', ...M4HF,
    flashKb: 512, ramKb: 64, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'NRF52832_XXAA', openocdTarget: 'nrf52', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'RADIO', 'BLE', 'NFC'],
  }),
  b({
    id: 'nrf52840', name: 'nRF52840', family: 'Nordic nRF52', mcu: 'nRF52840', ...M4HF,
    flashKb: 1024, ramKb: 256, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'NRF52840_XXAA', openocdTarget: 'nrf52', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'RADIO', 'BLE', '802.15.4'],
  }),
  b({
    id: 'nrf5340', name: 'nRF5340', family: 'Nordic nRF53', mcu: 'nRF5340', ...M33HF,
    flashKb: 1024, ramKb: 512, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'NRF5340_XXAA_APPLICATION', openocdTarget: 'nrf53', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'RADIO', 'BLE', 'DUAL_CORE'],
  }),
  b({
    id: 'nrf9160', name: 'nRF9160', family: 'Nordic nRF91', mcu: 'nRF9160', ...M33HF,
    flashKb: 1024, ramKb: 256, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'NRF9160_XXAA', openocdTarget: 'nrf91', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'LTE', 'GPS', 'TRUSTZONE'],
  }),

  // ── Infineon XMC / Cypress PSoC (industrial) ─────────────
  b({
    id: 'xmc4700', name: 'XMC4700-2048', family: 'Infineon XMC', mcu: 'XMC4700F144K2048', ...M4HF,
    flashKb: 2048, ramKb: 352, flashOrigin: '0x08000000', ramOrigin: '0x1FFE8000',
    cDefine: 'XMC4700_F144x2048', openocdTarget: 'xmc4xxx', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'DAC', 'ETH', 'CAN', 'USB'],
  }),
  b({
    id: 'cy8c624ab', name: 'PSoC 6 CY8C624AB', family: 'Infineon PSoC 6', mcu: 'CY8C624ABZI-D44', ...M4HF,
    flashKb: 2048, ramKb: 1024, flashOrigin: '0x10000000', ramOrigin: '0x08000000',
    cDefine: 'CY8C624ABZI_D44', openocdTarget: 'psoc6', defaultAdapter: 'kitprog3',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'USB', 'BLE', 'DUAL_CORE'],
  }),

  // ── Renesas RA (industrial / automotive-adjacent) ────────
  b({
    id: 'ra6m5', name: 'RA6M5', family: 'Renesas RA', mcu: 'R7FA6M5BH3CFC', ...M33HF,
    flashKb: 2048, ramKb: 512, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: 'BOARD_RA6M5_EK', openocdTarget: 'renesas_ra', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB', 'TRUSTZONE'],
  }),
  b({
    id: 'ra8d1', name: 'RA8D1', family: 'Renesas RA', mcu: 'R7KA8D1KFLCAC',
    cpu: 'cortex-m85', fpu: 'fpv5-d16', floatAbi: 'hard', rustTarget: 'thumbv8m.main-none-eabihf',
    flashKb: 2048, ramKb: 1280, flashOrigin: '0x02000000', ramOrigin: '0x22000000',
    cDefine: 'BOARD_RA8D1_EK', openocdTarget: 'renesas_ra', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'GLCDC', 'USB', 'TRUSTZONE'],
  }),

  // ── Microchip SAM (ATSAM — not Arduino boards) ───────────
  b({
    id: 'same54p20', name: 'ATSAME54P20', family: 'Microchip SAM E5x', mcu: 'ATSAME54P20A', ...M4HF,
    flashKb: 1024, ramKb: 256, flashOrigin: '0x00000000', ramOrigin: '0x20000000',
    cDefine: '__SAME54P20A__', openocdTarget: 'atsame5x', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB'],
  }),
  b({
    id: 'same70q21', name: 'ATSAME70Q21', family: 'Microchip SAM E70', mcu: 'ATSAME70Q21B', ...M7SP,
    flashKb: 2048, ramKb: 384, flashOrigin: '0x00400000', ramOrigin: '0x20400000',
    cDefine: '__SAME70Q21B__', openocdTarget: 'atsame70', defaultAdapter: 'jlink',
    peripherals: ['GPIO', 'UART', 'SPI', 'I2C', 'TIM', 'ADC', 'CAN', 'ETH', 'USB', 'ISI'],
  }),
]

const DEFAULT_BOARD_ID = 'stm32f407vg'

function listBoards() {
  return BOARDS.map(({ id, name, family, mcu, flashKb, ramKb, cpu }) => ({
    id, name, family, mcu, flashKb, ramKb, cpu,
  }))
}

function getBoard(boardId) {
  return BOARDS.find(b => b.id === boardId) || null
}

function getBoardOrDefault(boardId) {
  return getBoard(boardId) || getBoard(DEFAULT_BOARD_ID)
}

function cpuFlags(board) {
  const parts = [`-mcpu=${board.cpu}`, '-mthumb']
  if (board.fpu) {
    parts.push(`-mfpu=${board.fpu}`, `-mfloat-abi=${board.floatAbi || 'hard'}`)
  }
  return parts.join(' ')
}

function memLength(kb) {
  if (!kb || kb <= 0) return '0K'
  if (kb >= 1024 && kb % 1024 === 0) return `${kb / 1024}M`
  return `${kb}K`
}

module.exports = {
  BOARDS,
  DEFAULT_BOARD_ID,
  listBoards,
  getBoard,
  getBoardOrDefault,
  cpuFlags,
  memLength,
}
