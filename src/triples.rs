use crate::containers::{AdjList, Bitmap, Sequence};
use crate::object_iter::ObjectIter;
use crate::predicate_iter::PredicateIter;
use crate::ControlInfo;
use rsdict::RsDict;
use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::io;
use std::io::BufRead;
use sucds::WaveletMatrix;

#[derive(Debug, Clone)]
// TODO is anyone actually using other formats than triple bitmaps or can we remove the enum?
// The unnecessary matches make the code more verbose.
pub enum TripleSect {
    Bitmap(TriplesBitmap),
}

impl TripleSect {
    pub fn read<R: BufRead>(reader: &mut R) -> io::Result<Self> {
        use io::Error;
        use io::ErrorKind::InvalidData;
        let triples_ci = ControlInfo::read(reader)?;

        match &triples_ci.format[..] {
            "<http://purl.org/HDT/hdt#triplesBitmap>" => {
                Ok(TripleSect::Bitmap(TriplesBitmap::read(reader, triples_ci)?))
            }
            "<http://purl.org/HDT/hdt#triplesList>" => {
                Err(Error::new(InvalidData, "Triples Lists are not supported yet."))
            }
            _ => Err(Error::new(InvalidData, "Unknown triples listing format.")),
        }
    }

    pub fn read_all_ids(&self) -> BitmapIter {
        match self {
            TripleSect::Bitmap(bitmap) => bitmap.into_iter(),
        }
    }

    pub fn triples_with_s(&self, subject_id: usize) -> BitmapIter {
        match self {
            TripleSect::Bitmap(bitmap) => BitmapIter::with_s(bitmap, subject_id),
        }
    }

    pub fn triples_with_o(&self, object_id: usize) -> ObjectIter {
        match self {
            TripleSect::Bitmap(bitmap) => ObjectIter::new(bitmap, object_id),
        }
    }

    pub fn triples_with_p(&self, predicate_id: usize) -> PredicateIter {
        match self {
            TripleSect::Bitmap(bitmap) => PredicateIter::new(bitmap, predicate_id),
        }
    }
}

#[repr(u8)]
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Order {
    Unknown = 0,
    SPO = 1,
    SOP = 2,
    PSO = 3,
    POS = 4,
    OSP = 5,
    OPS = 6,
}

impl TryFrom<u32> for Order {
    type Error = std::io::Error;

    fn try_from(original: u32) -> Result<Self, Self::Error> {
        match original {
            0 => Ok(Order::Unknown),
            1 => Ok(Order::SPO),
            2 => Ok(Order::SOP),
            3 => Ok(Order::PSO),
            4 => Ok(Order::POS),
            5 => Ok(Order::OSP),
            6 => Ok(Order::OPS),
            _ => Err(Self::Error::new(io::ErrorKind::InvalidData, "Unrecognized order")),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TriplesBitmap {
    order: Order,
    pub adjlist_y: AdjList,
    pub adjlist_z: AdjList,
    pub op_index: AdjList,
    pub wavelet_y: WaveletMatrix,
}

impl TriplesBitmap {
    pub fn read<R: BufRead>(reader: &mut R, triples_ci: ControlInfo) -> io::Result<Self> {
        use std::io::Error;
        use std::io::ErrorKind::InvalidData;

        // read order
        let order: Order;
        if let Some(n) = triples_ci.get("order").and_then(|v| v.parse::<u32>().ok()) {
            order = Order::try_from(n)?;
        } else {
            return Err(Error::new(InvalidData, "Unrecognized order"));
        }

        // read bitmaps
        let bitmap_y = Bitmap::read(reader)?;
        let bitmap_z = Bitmap::read(reader)?;

        // read sequences
        let sequence_y = Sequence::read(reader)?;
        let sequence_z = Sequence::read(reader)?;

        // construct adjacency lists
        let adjlist_y = AdjList::new(sequence_y, bitmap_y);
        let adjlist_z = AdjList::new(sequence_z, bitmap_z);

        // construct object-based index to traverse from the leaves and support O?? queries
        // unfinished
        print!("Constructing OPS index");
        let entries = adjlist_z.sequence.entries;

        // Could import the multimap crate instead but not worth it for single use.
        // Alternatively, the index could be directly created using sequence which would save memory during index generation.
        // See https://github.com/rdfhdt/hdt-cpp/blob/develop/libhdt/src/triples/BitmapTriples.cpp
        let mut map = BTreeMap::<usize, Vec<usize>>::new();

        // Count the number of appearances of each object
        for i in 0..entries - 1 {
            let object = adjlist_z.sequence.get(i);
            if object == 0 {
                eprintln!("ERROR: There is a zero value in the Z level.");
                continue;
            }
            /*
             if object == 162 {
                println!("162 found at adjlist_z pos {}",i);
            }
            */
            if let Some(indexes) = map.get_mut(&object) {
                indexes.push(i); // hdt index counts from 1 but we count from 0 for simplicity
            } else {
                map.insert(object, vec![i]);
            }
        }
        //println!("pos for 162 {:?}", map.get(&162));

        // reduce memory consumption of index by using adjacency list
        let mut bitmap_index_dict = RsDict::new();

        let mut sequence_index_data = Vec::<usize>::new();
        for (object, indexes) in map {
            let mut first = true;
            for index in indexes {
                bitmap_index_dict.push(first);
                first = false;
                sequence_index_data.push(index);
            }
        }
        /*
                println!("index test index pos {}",bitmap_index_dict.select1(162-1).unwrap());
                println!("index test {}",sequence_index_data.get(bitmap_index_dict.select1(162-1).unwrap() as usize).unwrap());
                println!("adjlist_z there {}",adjlist_z.sequence.get(91328));
                let y_pos = adjlist_z.bitmap.dict.rank(91328,true) as usize;
                println!("y_pos {}",y_pos);
                let y = adjlist_y.sequence.get(y_pos);
                println!("y {}",y);
        */

        let bitmap_index = Bitmap { dict: bitmap_index_dict };
        // we don't actually save space with 64 bits TODO compress into lower bit sequence
        let sequence_index = Sequence {
            entries,
            bits_per_entry: 64, //(entries as f64).log2().ceil() as usize, // is this correct?
            data: sequence_index_data,
        };
        /*
        println!(
            "sequence index test {}",
            sequence_index.get(bitmap_index.dict.select1(162 - 1).unwrap() as usize)
        );
        */
        let op_index = AdjList::new(sequence_index, bitmap_index);
        println!("...finished constructing OPS index");
        print!("Start constructing wavelet matrix");
        let wavelet_y = WaveletMatrix::from_ints(adjlist_y.sequence.into_iter()).unwrap();
        println!("...finished constructing wavelet matrix with length {}", wavelet_y.len());
        Ok(TriplesBitmap { order, adjlist_y, adjlist_z, op_index, wavelet_y })
    }

    pub fn coord_to_triple(&self, x: usize, y: usize, z: usize) -> io::Result<TripleId> {
        use io::Error;
        use io::ErrorKind::InvalidData;
        if x == 0 || y == 0 || z == 0 {
            return Err(Error::new(
                InvalidData,
                format!("({},{},{}) none of the components of a triple may be 0.", x, y, z),
            ));
        }
        match self.order {
            Order::SPO => Ok(TripleId::new(x, y, z)),
            Order::SOP => Ok(TripleId::new(x, z, y)),
            Order::PSO => Ok(TripleId::new(y, x, z)),
            Order::POS => Ok(TripleId::new(y, z, x)),
            Order::OSP => Ok(TripleId::new(z, x, y)),
            Order::OPS => Ok(TripleId::new(z, y, x)),
            Order::Unknown => Err(Error::new(InvalidData, "unknown triples order")),
        }
    }

    /// Martinez et al. 2012 page 8
    pub fn find_subj(&self, p: usize) //-> (usize,impl Iterator<usize>)
    {
        //let occs = adjlist_y.sequence.select...
    }

    //pub fn find_ob
}
/*
// old iterator without references
impl IntoIterator for TriplesBitmap {
    type Item = TripleId;
    type IntoIter = BitmapIter;

    fn into_iter(self) -> Self::IntoIter {
        BitmapIter::new(self)
    }
}
*/
impl<'a> IntoIterator for &'a TriplesBitmap {
    type Item = TripleId;
    type IntoIter = BitmapIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        BitmapIter::new(self)
    }
}

pub struct BitmapIter<'a> {
    // triples data
    triples: &'a TriplesBitmap,
    // x-coordinate identifier
    x: usize,
    // current position
    pos_y: usize,
    pos_z: usize,
    max_y: usize,
    max_z: usize,
}

impl<'a> BitmapIter<'a> {
    pub fn new(triples: &'a TriplesBitmap) -> Self {
        BitmapIter {
            x: 1, // was 0 in the old code but it should start at 1
            pos_y: 0,
            pos_z: 0,
            max_y: triples.adjlist_y.len(), // exclusive
            max_z: triples.adjlist_z.len(), // exclusive
            triples,
        }
    }

    /// see <https://github.com/rdfhdt/hdt-cpp/blob/develop/libhdt/src/triples/BitmapTriplesIterators.cpp>
    pub fn with_s(triples: &'a TriplesBitmap, subject_id: usize) -> Self {
        /*let ay = &triples.adjlist_y;
        for i in 1..3 {
        println!("{} {}",ay.find(i), ay.last(i));
        }*/
        let min_y = triples.adjlist_y.find(subject_id - 1);
        let min_z = triples.adjlist_z.find(min_y);
        let max_y = triples.adjlist_y.find(subject_id);
        let max_z = triples.adjlist_z.find(max_y);
        /*println!(
            "BitMapIter::with_s subject_id={} min_y={} max_y={} min_z={} max_z={}",
            subject_id, min_y, max_y, min_z, max_z
        );*/
        BitmapIter { triples, x: subject_id, pos_y: min_y, pos_z: min_z, max_y, max_z }
    }
}

impl<'a> Iterator for BitmapIter<'a> {
    type Item = TripleId;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos_y >= self.max_y {
            return None;
        }

        if self.pos_z >= self.max_z {
            return None;
        }

        //println!("{} pos {} {} ",self.x,self.pos_y,self.pos_z);
        let y = self.triples.adjlist_y.get_id(self.pos_y);
        let z = self.triples.adjlist_z.get_id(self.pos_z);
        //println!("x y z {} {} {}", self.x,y,z);
        //println!("{} {}", self.triples.adjlist_y.at_last_sibling(self.pos_y), self.triples.adjlist_z.at_last_sibling(self.pos_z));
        let triple_id = self.triples.coord_to_triple(self.x, y, z).unwrap();

        // theoretically the second condition should only be true if the first is as well but in practise it wasn't, which screwed up the subject identifiers
        // fixed by moving the second condition inside the first one but there may be another reason for the bug occuring in the first place
        if self.triples.adjlist_z.at_last_sibling(self.pos_z) {
            if self.triples.adjlist_y.at_last_sibling(self.pos_y) {
                self.x += 1;
            }
            self.pos_y += 1;
        }
        //if ! self.triples.adjlist_y.at_last_sibling(self.pos_y) && (!self.triples.adjlist_z.at_last_sibling(self.pos_z)) {self.x-=1;}

        self.pos_z += 1;

        Some(triple_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct TripleId {
    pub subject_id: usize,
    pub predicate_id: usize,
    pub object_id: usize,
}

impl TripleId {
    pub fn new(subject_id: usize, predicate_id: usize, object_id: usize) -> Self {
        TripleId { subject_id, predicate_id, object_id }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlInfo, Dict, Header};
    use std::fs::File;
    use std::io::BufReader;

    #[test]
    fn read_triples() {
        let file = File::open("tests/resources/swdf.hdt").expect("error opening file");
        let mut reader = BufReader::new(file);
        ControlInfo::read(&mut reader).unwrap();
        let _header = Header::read(&mut reader).unwrap();
        Dict::read(&mut reader).unwrap();
        let triples = TripleSect::read(&mut reader).unwrap();
        let v: Vec<TripleId> = triples.read_all_ids().into_iter().collect::<Vec<TripleId>>();
        assert_eq!(v.len(), 242256);
        //println!("{:#?}", &v[0..30]);
        assert_eq!(v[0].subject_id, 1);
        assert_eq!(v[2].subject_id, 1);
        assert_eq!(v[3].subject_id, 2);
        //for i in 1..200  {println!("{:?}",(&v).into_iter().filter(|tid| tid.object_id == i).collect::<Vec<&TripleId>>());}
        let triples_with_s = [
            vec![(1, 90, 13304), (1, 101, 19384), (1, 111, 75817)],
            vec![(5, 90, 13017), (5, 101, 14748), (5, 111, 75817)],
            vec![(7, 90, 15802), (7, 101, 15758), (7, 104, 17490), (7, 105, 18547), (7, 111, 75817)],
        ];
        // theorectially order doesn't matter so should derive Hash for TripleId and use HashSet but not needed in practice
        for ts in triples_with_s {
            assert_eq!(
                ts.clone().into_iter().map(|(x, y, z)| TripleId::new(x, y, z)).collect::<Vec<TripleId>>(),
                triples.triples_with_s(ts[0].0).collect::<Vec<TripleId>>()
            );
        }

        let triples_with_o = [
            vec![(7077, 129, 162), (12288, 150, 162), (23261, 18, 162)],
            vec![(7088, 129, 184), (19818, 18, 184)],
            vec![(1364, 14, 193)],
        ];
        for to in triples_with_o {
            let tids = to.clone().into_iter().map(|(x, y, z)| TripleId::new(x, y, z)).collect::<Vec<TripleId>>();
            //println!("{:?}", tids);
            assert_eq!(tids, triples.triples_with_o(to[0].2).collect::<Vec<TripleId>>());
        }

        //for i in 1..150 {println!("{:?}", (&v).into_iter().filter(|tid| tid.predicate_id == i).collect::<Vec<&TripleId>>());}
        println!("{:?}", (&v).into_iter().filter(|tid| tid.predicate_id == 4).collect::<Vec<&TripleId>>());

        let triples_with_p = [vec![(3232, 4, 3233), (3545, 4, 3643), (3642, 4, 3643), (6551, 4, 67719)]];
        for to in triples_with_p {
            let tids = to.clone().into_iter().map(|(x, y, z)| TripleId::new(x, y, z)).collect::<Vec<TripleId>>();
            //println!("{:?}", tids);
            assert_eq!(tids, triples.triples_with_p(to[0].1).collect::<Vec<TripleId>>());
        }
    }
}
