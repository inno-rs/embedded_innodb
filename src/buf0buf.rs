pub struct BufBlock {}

impl BufBlock {
    pub fn getFrame(&self) -> [u8; 4096] {
        let mut frame: [u8; 4096] = [0; 4096];
        // Fill the frame with some data
        for i in 0..4096 {
            frame[i] = i as u8;
        }

        frame
    }
}
