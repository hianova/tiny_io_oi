use tiny_io_oi::{TinyNode, OpCode};
use tiny_io_oi::drivers::{MockNetwork, MockMotor, MockState, MockGpio};

fn main() {
    let my_id = [0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    let leader_id = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
    
    let mut network = MockNetwork::new();
    let motor = MockMotor::new();
    let state = MockState::default();
    let gpio = MockGpio::new();
    
    // Simulate receiving a HEARTBEAT from the leader
    network.simulate_receive(leader_id, OpCode::Heartbeat, &[]);
    
    // Simulate receiving a TASK_DISPATCH from the leader
    network.simulate_receive(leader_id, OpCode::TaskDispatch, &[]);

    // Simulate receiving a STATE_UPDATE (bitmask 0x01)
    network.simulate_receive(leader_id, OpCode::StateUpdate, &[0x01]);
    
    let mut node = TinyNode::<_, _, _, _, 5>::new(my_id, network, motor, state, gpio);
    
    println!("--- Step 1: Processing Heartbeat ---");
    node.tick();
    
    println!("--- Step 2: Processing Task Dispatch (Motor should turn on) ---");
    node.tick();

    println!("--- Step 3: Processing State Update (Flags should change) ---");
    node.tick();
    
    println!("--- Finished Demo ---");
}
