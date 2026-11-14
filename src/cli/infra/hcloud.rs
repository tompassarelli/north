//! Hetzner Cloud servers, read through the credential broker's `hcloud` proxy. `/v1/servers` already carries each
//! server's type and that type's monthly gross price per location, so one GET lists what is live and what it costs.
use serde_json::Value;

use super::report::{Price, Resource};

pub const PATH: &str = "/hcloud/v1/servers";

pub fn servers(answer: &Value) -> Result<Vec<Resource>, String> {
    let servers = answer["servers"].as_array().ok_or("the hcloud answer holds no servers list")?;
    Ok(servers.iter().map(server).collect())
}

fn server(server: &Value) -> Resource {
    let name = server["name"].as_str().unwrap_or("unnamed").to_owned();
    let location = server["location"]["name"].as_str().map(str::to_owned);
    let status = server["status"].as_str().unwrap_or_default();
    Resource::new("hetzner", name, "server").at(location.clone()).status(status).priced(monthly(server, location.as_deref()))
}

/// The type's gross monthly price at the server's own location, else its first listed location, as hcloud reports gross.
fn monthly(server: &Value, location: Option<&str>) -> Option<Price> {
    let prices = server["server_type"]["prices"].as_array()?;
    let entry = prices.iter().find(|entry| entry["location"].as_str() == location).or_else(|| prices.first())?;
    let gross = &entry["price_monthly"]["gross"];
    let amount = gross.as_str().and_then(|text| text.parse::<f64>().ok()).or_else(|| gross.as_f64())?;
    Some(Price::eur(amount))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_server_prices_at_its_own_location_not_the_first_one_listed() {
        let answer = json!({"servers": [{
            "name": "cpx62",
            "status": "running",
            "location": {"name": "hel1"},
            "server_type": {"prices": [
                {"location": "fsn1", "price_monthly": {"gross": "152.9900000000"}},
                {"location": "hel1", "price_monthly": {"gross": "152.9900000000"}},
                {"location": "sin", "price_monthly": {"gross": "202.9900000000"}}
            ]}
        }]});
        let servers = servers(&answer).expect("the answer parses");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "cpx62");
        assert_eq!(servers[0].location.as_deref(), Some("hel1"));
        assert_eq!(servers[0].price, Some(Price::eur(152.99)));
    }

    #[test]
    fn a_server_without_a_price_still_lists_so_its_status_shows() {
        let answer = json!({"servers": [{"name": "cpx62", "status": "off", "server_type": {"prices": []}}]});
        let servers = servers(&answer).expect("the answer parses");
        assert_eq!(servers[0].price, None);
        assert_eq!(servers[0].status, "off");
    }

    #[test]
    fn an_answer_without_a_servers_list_is_an_error_rather_than_an_empty_infra() {
        assert!(servers(&json!({"error": "nope"})).is_err());
    }
}
