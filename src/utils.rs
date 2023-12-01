use crate::{Error, Result};
use iptools::iprange::{IpRange, IpVer};

// TODO fix bug
pub fn ipv4_cidr_to_wildcard(cidr: &str) -> Result<Vec<String>> {
    let ip = IpRange::new(cidr, "").or(Err(Error::ParseStr(cidr.into())))?;

    if ip.get_version() != IpVer::IPV4 {
        return Err(Error::ParseStr(cidr.into()));
    }

    let (start, end) = ip.get_range().unwrap(); // It must be cidr, so unwrap is safe.
    let start = start.split('.').collect::<Vec<&str>>();
    let end = end.split('.').collect::<Vec<&str>>();

    println!("start: {:?}, end: {:?}", start, end);
    let mut ret = vec![];
    let mut each = String::new();
    for i in 0..4 {
        if start[i] == end[i] {
            each.push_str(start[i]);
            if i != 3 {
                each.push('.');
            }
            continue;
        }

        if start[i] == "0" && end[i] == "255" {
            each.push('*');
            println!("111 {each}    {i}");
            ret.push(each);
            break;
        }

        let s = start[i]
            .parse::<u8>()
            .or(Err(Error::ParseStr(cidr.into())))?;
        let e = end[i].parse::<u8>().or(Err(Error::ParseStr(cidr.into())))?;

        for j in s..e + 1 {
            let mut builder = each.clone();
            builder.push_str(&j.to_string());
            if i != 3 {
                builder.push_str(".*");
            }
            println!("222 {}", builder);
            ret.push(builder);
        }
    }
    Ok(ret)
}

#[test]
fn test() {
    // 给translate_passby写测试用例
    let c1 = "127.0.0.1/6";
    let c2 = "127.0.0.1/8";

    println!("{:?}", ipv4_cidr_to_wildcard(c1));
    println!("{:?}", ipv4_cidr_to_wildcard(c2));
}
