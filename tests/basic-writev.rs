use std::fs::File;
use std::io::Read; 

use ringbahn::Submission;
use ringbahn::event::WriteV;
use ringbahn::drive::demo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn writev_file() {
    let mut file = tempfile::tempfile().unwrap();
    let vec1 = &ASSERT[0..4];
    let vec2 = &ASSERT[4..9];
    let vec3 = &ASSERT[9..];
    let writev: WriteV<'_, File> = WriteV::new(&file, vec![vec1.into(), vec2.into(), vec3.into()], 0);
    let (_, result) = futures::executor::block_on(Submission::new(writev, demo::driver()));
    assert_eq!(result.unwrap(), ASSERT.len());

    let mut buf = vec![];
    assert_eq!(file.read_to_end(&mut buf).unwrap(), ASSERT.len());
    assert_eq!(&buf[0..ASSERT.len()], ASSERT);
}
