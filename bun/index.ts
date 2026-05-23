import { cc, dlopen, ptr, suffix } from "bun:ffi";
import { join } from "path";

// Locate the dynamic library compiled by Rust
const libPath = join(import.meta.dir, "../target/debug/libtiny_io_oi." + suffix);

// Define symbols for C FFI bindings under Bun
export const { symbols } = dlopen(libPath, {
  create_script_builder: {
    args: [],
    returns: "ptr",
  },
  script_builder_add_pwm: {
    args: ["ptr", "u8", "u8"],
    returns: "void",
  },
  script_builder_add_delay: {
    args: ["ptr", "u32"],
    returns: "void",
  },
  script_builder_add_assert: {
    args: ["ptr", "u8", "u8"],
    returns: "void",
  },
  script_builder_serialize: {
    args: ["ptr", "ptr"], // ScriptBuilder pointer, out_len pointer
    returns: "ptr",       // returns *mut u8
  },
  script_builder_free: {
    args: ["ptr"],
    returns: "void",
  },
  free_serialized_bytes: {
    args: ["ptr", "u32"],
    returns: "void",
  },
  rust_fft: {
    args: ["ptr", "u32", "f32", "ptr"], // input_ptr, len, sample_rate, output_ptr
    returns: "f32",                    // returns peak frequency
  },
  run_standard_bytecode: {
    args: ["ptr", "u32", "u32", "u8", "u8", "ptr"], // bytecode_ptr, len, fuel, initial_motor_speed, pin_sensor_val, out_final_motor_speed_ptr
    returns: "i32",
  },
});

/**
 * A Bun-compatible FFI builder to accumulate and compile VmScript bytecode streams
 * in a high-speed, memory-safe, and zero-allocation manner.
 */
export class ScriptBuilder {
  private ptr: any;

  constructor() {
    this.ptr = symbols.create_script_builder();
    if (!this.ptr) {
      throw new Error("Failed to instantiate ScriptBuilder in Rust FFI");
    }
  }

  /**
   * Add a instruction to set physical PWM speed on a channel.
   */
  public setPwm(channel: number, speed: number): this {
    symbols.script_builder_add_pwm(this.ptr, channel, speed);
    return this;
  }

  /**
   * Add a delay instruction for busy-waiting.
   */
  public delay(ticks: number): this {
    symbols.script_builder_add_delay(this.ptr, ticks);
    return this;
  }

  /**
   * Add a physical assertion logic check.
   */
  public assertOrYield(pin: number, expected: boolean | number): this {
    const expectedVal = typeof expected === "boolean" ? (expected ? 1 : 0) : expected;
    symbols.script_builder_add_assert(this.ptr, pin, expectedVal);
    return this;
  }

  /**
   * Compiles the accumulated instructions into a high-performance rkyv-aligned Uint8Array binary.
   */
  public serialize(): Uint8Array {
    if (!this.ptr) {
      throw new Error("ScriptBuilder is already freed");
    }
    const lenBuf = new Uint32Array(1);
    const lenPtr = ptr(lenBuf);
    const bytesPtr = symbols.script_builder_serialize(this.ptr, lenPtr);

    if (!bytesPtr) {
      throw new Error("Failed to serialize script bytecode in Rust");
    }

    const len = lenBuf[0];
    
    // Safety check and copy the FFI binary memory into JS-managed TypedArray
    const srcView = new Uint8Array(Bun.ArrayBuffer.fromAddress(bytesPtr, len));
    const copied = new Uint8Array(srcView);

    // Free the Rust heap-allocated serialized bytes immediately to prevent memory leaks
    symbols.free_serialized_bytes(bytesPtr, len);

    return copied;
  }

  /**
   * Deallocates the Rust-side builder.
   */
  public free(): void {
    if (this.ptr) {
      symbols.script_builder_free(this.ptr);
      this.ptr = null;
    }
  }
}

/**
 * TS/JS Facade representing an Asymmetric Swarm Endpoint (BAV, actuator, Swarm node).
 * Exposes clean TS interfaces to compile and deploy dynamic logic.
 */
export class TinyNode {
  private nodeId: number;

  constructor(nodeId: number) {
    this.nodeId = nodeId;
  }

  public getNodeId(): number {
    return this.nodeId;
  }

  /**
   * High-level fluent builder API to compile hardware scripts on-the-fly.
   */
  public buildScript(callback: (builder: ScriptBuilder) => void): Uint8Array {
    const builder = new ScriptBuilder();
    try {
      callback(builder);
      return builder.serialize();
    } finally {
      builder.free();
    }
  }
}

// =========================================================================
// Semantic Hardware Reflex Standard Library (tiny_io_oi::std v1.0 & v1.1)
// =========================================================================

export enum LogicOp {
  AND = 0,
  OR = 1,
}

export enum Band {
  LOW = 0x01,
  MID = 0x02,
  HIGH = 0x04,
}

/**
 * High-performance 8-byte fixed-length semantic bytecode builder for the Standard Library.
 * Seamlessly interfaces with Rust's no_std定點數FFT, active resonance avoiders, and multi-band checks.
 */
export class TinyScriptBuilder {
  private buffer: Uint8Array;
  private offset: number = 0;

  constructor(maxSteps: number = 32) {
    this.buffer = new Uint8Array(maxSteps * 8);
  }

  /**
   * 0x80: AssertVibration (震動安全斷言)
   * 
   * Triggers emergency Safe Shutdown if dominant frequency exceeds maxHz AND peak magnitude exceeds threshold.
   * 
   * @param pin GPIO pin for the sensor.
   * @param maxHz The maximum allowed frequency (Hz).
   * @param threshold Q15 amplitude threshold (0.0 ~ 2.0).
   */
  public assertVibration(pin: number, maxHz: number, threshold: number): this {
    const view = new DataView(this.buffer.buffer);
    const thresholdQ15 = Math.min(Math.max(threshold * 32768, 0), 65535);

    view.setUint8(this.offset, 0x80);
    view.setUint8(this.offset + 1, pin);
    view.setUint16(this.offset + 2, thresholdQ15, true); // Parameter A
    view.setUint32(this.offset + 4, maxHz, true);        // Parameter B

    this.offset += 8;
    return this;
  }

  /**
   * 0x81: AvoidResonance (共振規避/主動調頻)
   * 
   * Actively shifts motor speed if physical vibration approaches structural resonance frequencies.
   */
  public avoidResonance(options: {
    sensorPin: number;
    motorChannel: number;
    resonanceHz: number;
    toleranceHz: number;
  }): this {
    const view = new DataView(this.buffer.buffer);

    // Pack Parameter B: [Tolerance (1B)] [MotorChannel (1B)] [Reserved (2B)]
    const paramB = ((options.toleranceHz & 0xFF) << 24) |
                   ((options.motorChannel & 0xFF) << 16);

    view.setUint8(this.offset, 0x81);
    view.setUint8(this.offset + 1, options.sensorPin);
    view.setUint16(this.offset + 2, options.resonanceHz, true); // Parameter A
    view.setUint32(this.offset + 4, paramB, true);              // Parameter B

    this.offset += 8;
    return this;
  }

  /**
   * 0x83: MultiBandAssert (多頻段複合斷言)
   * 
   * Perform logical combinations (AND/OR) of Low, Mid, and High band vibration checks.
   */
  public assertMultiBand(
    pin: number,
    bands: number,
    op: LogicOp,
    lowMidThreshold: number,
    highThreshold: number
  ): this {
    const view = new DataView(this.buffer.buffer);

    // Pack Parameter A: [LogicOp (1B)] [BandsMask (1B)]
    const paramA = (op << 8) | (bands & 0xFF);
    const lowMidQ15 = Math.min(lowMidThreshold * 32768, 65535);
    const highQ15 = Math.min(highThreshold * 32768, 65535);
    // Pack Parameter B: [LowMidQ15 (2B)] [HighQ15 (2B)]
    const paramB = (lowMidQ15 << 16) | (highQ15 & 0xFFFF);

    view.setUint8(this.offset, 0x83);
    view.setUint8(this.offset + 1, pin);
    view.setUint16(this.offset + 2, paramA, true);
    view.setUint32(this.offset + 4, paramB, true);

    this.offset += 8;
    return this;
  }

  /**
   * 0x84: SpectrumAdaptive (頻譜自適應多段控制)
   * 
   * Decision tree based routing: Load imbalance -> Throttle speed; Resonance -> Shifting; Acoustic friction -> Shutdown.
   */
  public adaptiveControl(options: {
    sensorPin: number;
    motorChannel: number;
    toleranceHz: number;
    lowThreshold: number;
    midThreshold: number;
  }): this {
    const view = new DataView(this.buffer.buffer);

    const paramA = (options.motorChannel << 8) | (options.toleranceHz & 0xFF);
    const lowQ15 = Math.min(options.lowThreshold * 32768, 65535);
    const midQ15 = Math.min(options.midThreshold * 32768, 65535);
    const paramB = (lowQ15 << 16) | (midQ15 & 0xFFFF);

    view.setUint8(this.offset, 0x84);
    view.setUint8(this.offset + 1, options.sensorPin);
    view.setUint16(this.offset + 2, paramA, true);
    view.setUint32(this.offset + 4, paramB, true);

    this.offset += 8;
    return this;
  }

  /**
   * 0x85: EnvelopeCheck (頻譜包絡線檢測)
   */
  public checkEnvelope(pin: number, profileId: number, maxViolations: number): this {
    const view = new DataView(this.buffer.buffer);

    view.setUint8(this.offset, 0x85);
    view.setUint8(this.offset + 1, pin);
    view.setUint16(this.offset + 2, profileId, true);
    view.setUint32(this.offset + 4, maxViolations, true);

    this.offset += 8;
    return this;
  }

  /**
   * Serializes the standard library steps into raw byte array.
   */
  public serialize(): Uint8Array {
    return this.buffer.slice(0, this.offset);
  }
}
