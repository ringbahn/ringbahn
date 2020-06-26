use std::fs::File;

use ringbahn::Submission;
use ringbahn::event::ReadV;
use ringbahn::drive::demo;

const ASSERT: &[u8] = b"But this formidable power of death -";

#[test]
fn readv_file() {
    let file = File::open("props.txt").unwrap();
    let vec1 = vec![0; 4];
    let vec2 = vec![0; 5];
    let vec3 = vec![0; 10];
    let readv: ReadV<'_, File> = ReadV::new(&file, vec![vec1, vec2, vec3], 0);
    let (readv, result) = futures::executor::block_on(Submission::new(readv, demo::driver()));
    dbg!(&result); 
    assert!(result.is_ok());
    assert_eq!(readv.bufs[0][..], ASSERT[0..4]); 
    assert_eq!(readv.bufs[1][..], ASSERT[4..9]); 
    assert_eq!(readv.bufs[2][..], ASSERT[9..19]); 
}
