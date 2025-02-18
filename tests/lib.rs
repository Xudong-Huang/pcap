extern crate libc;
extern crate pcap;
extern crate tempdir;

use std::io;
use std::ops::Add;
use std::path::Path;
use tempdir::TempDir;

use pcap::{Activated, Active, Capture, Error, Linktype, Offline, Packet, PacketHeader, Precision};

#[test]
fn read_packet_with_full_data() {
    let mut capture = capture_from_test_file("packet_snaplen_65535.pcap");
    assert_eq!(capture.next().unwrap().len(), 98);
}

#[test]
fn read_packet_with_truncated_data() {
    let mut capture = capture_from_test_file("packet_snaplen_20.pcap");
    assert_eq!(capture.next().unwrap().len(), 20);
}

fn capture_from_test_file(file_name: &str) -> Capture<Offline> {
    let path = Path::new("tests/data/").join(file_name);
    Capture::from_file(path).unwrap()
}

#[test]
fn unify_activated() {
    #![allow(dead_code)]
    fn test1() -> Capture<Active> {
        loop {}
    }

    fn test2() -> Capture<Offline> {
        loop {}
    }

    fn maybe(a: bool) -> Capture<dyn Activated> {
        if a {
            test1().into()
        } else {
            test2().into()
        }
    }

    fn also_maybe(a: &mut Capture<dyn Activated>) {
        a.filter("whatever filter string, this won't be run anyway")
            .unwrap();
    }
}

#[derive(Clone)]
pub struct Packets {
    headers: Vec<PacketHeader>,
    data: Vec<Vec<u8>>,
}

impl Packets {
    pub fn new() -> Packets {
        Packets {
            headers: vec![],
            data: vec![],
        }
    }

    pub fn push(
        &mut self,
        tv_sec: libc::time_t,
        tv_usec: libc::suseconds_t,
        caplen: u32,
        len: u32,
        data: &[u8],
    ) {
        self.headers.push(PacketHeader {
            ts: libc::timeval {
                tv_sec: tv_sec,
                tv_usec: tv_usec,
            },
            caplen: caplen,
            len: len,
        });
        self.data.push(data.to_vec());
    }

    pub fn foreach<F: FnMut(&Packet)>(&self, mut f: F) {
        for (header, data) in self.headers.iter().zip(self.data.iter()) {
            let packet = Packet::new(header, &data);
            f(&packet);
        }
    }

    pub fn verify<T: Activated + ?Sized>(&self, cap: &mut Capture<T>) {
        for (header, data) in self.headers.iter().zip(self.data.iter()) {
            assert_eq!(cap.next().unwrap(), Packet::new(header, &data));
        }
        assert!(cap.next().is_err());
    }
}

impl<'a> Add for &'a Packets {
    type Output = Packets;

    fn add(self, rhs: &'a Packets) -> Packets {
        let mut packets = self.clone();
        packets.headers.extend(rhs.headers.iter());
        packets.data.extend(rhs.data.iter().cloned());
        packets
    }
}

#[test]
fn capture_dead_savefile() {
    let mut packets = Packets::new();
    packets.push(1460408319, 1234, 1, 1, &[1]);
    packets.push(1460408320, 4321, 1, 1, &[2]);

    let dir = TempDir::new("pcap").unwrap();
    let tmpfile = dir.path().join("test.pcap");

    let cap = Capture::dead(Linktype(1)).unwrap();
    let mut save = cap.savefile(&tmpfile).unwrap();
    packets.foreach(|p| save.write(p));
    drop(save);

    let mut cap = Capture::from_file(&tmpfile).unwrap();
    packets.verify(&mut cap);
}

#[test]
#[cfg(feature = "pcap-savefile-append")]
fn capture_dead_savefile_append() {
    let mut packets1 = Packets::new();
    packets1.push(1460408319, 1234, 1, 1, &[1]);
    packets1.push(1460408320, 4321, 1, 1, &[2]);
    let mut packets2 = Packets::new();
    packets2.push(1460408321, 2345, 1, 1, &[3]);
    packets2.push(1460408322, 5432, 1, 1, &[4]);
    let packets = &packets1 + &packets2;

    let dir = TempDir::new("pcap").unwrap();
    let tmpfile = dir.path().join("test.pcap");

    let cap = Capture::dead(Linktype(1)).unwrap();
    let mut save = cap.savefile(&tmpfile).unwrap();
    packets1.foreach(|p| save.write(p));
    drop(save);

    let cap = Capture::dead(Linktype(1)).unwrap();
    let mut save = cap.savefile_append(&tmpfile).unwrap();
    packets2.foreach(|p| save.write(p));
    drop(save);

    let mut cap = Capture::from_file(&tmpfile).unwrap();
    packets.verify(&mut cap);
}

#[test]
#[cfg(not(windows))]
fn test_raw_fd_api() {
    use std::fs::File;
    use std::io::prelude::*;
    #[cfg(not(windows))]
    use std::os::unix::io::{FromRawFd, RawFd};
    use std::thread;

    // Create a total of more than 64K data (> max pipe buf size)
    const N_PACKETS: usize = 64;
    let data: Vec<u8> = (0..191).cycle().take(N_PACKETS * 1024).collect();
    let mut packets = Packets::new();
    for i in 0..N_PACKETS {
        packets.push(
            1460408319 + i as libc::time_t,
            1000 + i as libc::suseconds_t,
            1024,
            1024,
            &data[i * 1024..(i + 1) * 1024],
        );
    }

    let dir = TempDir::new("pcap").unwrap();
    let tmpfile = dir.path().join("test.pcap");

    // Write all packets to test.pcap savefile
    let cap = Capture::dead(Linktype(1)).unwrap();
    let mut save = cap.savefile(&tmpfile).unwrap();
    packets.foreach(|p| save.write(p));
    drop(save);

    assert_eq!(
        Capture::from_raw_fd(-999).err().unwrap(),
        Error::InvalidRawFd
    );
    #[cfg(feature = "pcap-fopen-offline-precision")]
    {
        assert_eq!(
            Capture::from_raw_fd_with_precision(-999, Precision::Micro)
                .err()
                .unwrap(),
            Error::InvalidRawFd
        );
    }
    assert_eq!(
        cap.savefile_raw_fd(-999).err().unwrap(),
        Error::InvalidRawFd
    );

    // Create an unnamed pipe
    let mut pipe = [0 as libc::c_int; 2];
    assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
    let (fd_in, fd_out) = (pipe[0], pipe[1]);

    let filename = dir.path().join("test2.pcap");
    let packets_c = packets.clone();
    thread::spawn(move || {
        // Write all packets to the pipe
        let cap = Capture::dead(Linktype(1)).unwrap();
        let mut save = cap.savefile_raw_fd(fd_out).unwrap();
        packets_c.foreach(|p| save.write(p));
        // fd_out will be closed by savefile destructor
    });

    // Save the pcap from pipe in a separate thread.
    // Hypothetically, we could do any sort of processing here,
    // like encoding to a gzip stream.
    let mut file_in = unsafe { File::from_raw_fd(fd_in) };
    let mut file_out = File::create(&filename).unwrap();
    io::copy(&mut file_in, &mut file_out).unwrap();

    // Verify that the contents match
    let filename = dir.path().join("test2.pcap");
    let (mut v1, mut v2) = (vec![], vec![]);
    File::open(&tmpfile).unwrap().read_to_end(&mut v1).unwrap();
    File::open(&filename).unwrap().read_to_end(&mut v2).unwrap();
    assert_eq!(v1, v2);

    #[cfg(feature = "pcap-fopen-offline-precision")]
    fn from_raw_fd_with_precision(fd: RawFd, precision: Precision) -> Capture<Offline> {
        Capture::from_raw_fd_with_precision(fd, precision).unwrap()
    }

    #[cfg(not(feature = "pcap-fopen-offline-precision"))]
    fn from_raw_fd_with_precision(fd: RawFd, _: Precision) -> Capture<Offline> {
        Capture::from_raw_fd(fd).unwrap()
    }

    for with_tstamp in &[false, true] {
        // Create an unnamed pipe
        let mut pipe = [0 as libc::c_int; 2];
        assert_eq!(unsafe { libc::pipe(pipe.as_mut_ptr()) }, 0);
        let (fd_in, fd_out) = (pipe[0], pipe[1]);

        let filename = tmpfile.clone();
        thread::spawn(move || {
            // Cat the pcap into the pipe in a separate thread.
            // Hypothetically, we could do any sort of processing here,
            // like decoding from a gzip stream.
            let mut file_in = File::open(&filename).unwrap();
            let mut file_out = unsafe { File::from_raw_fd(fd_out) };
            io::copy(&mut file_in, &mut file_out).unwrap();
            assert_eq!(unsafe { libc::close(fd_out) }, 0);
        });

        // Open the capture with pipe's file descriptor
        let mut cap = if *with_tstamp {
            from_raw_fd_with_precision(fd_in, Precision::Micro)
        } else {
            Capture::from_raw_fd(fd_in).unwrap()
        };

        // Verify that packets match
        packets.verify(&mut cap);
    }
}

#[test]
fn test_linktype() {
    let capture = capture_from_test_file("packet_snaplen_65535.pcap");
    let linktype = capture.get_datalink();

    assert!(linktype.get_name().is_ok());
    assert_eq!(linktype.get_name().unwrap(), String::from("EN10MB"));
    assert!(linktype.get_description().is_ok());
}

#[test]
fn test_error() {
    let mut capture = capture_from_test_file("packet_snaplen_65535.pcap");
    // Trying to get stats from offline capture should error.
    assert!(capture.stats().err().is_some());
}
