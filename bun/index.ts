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
