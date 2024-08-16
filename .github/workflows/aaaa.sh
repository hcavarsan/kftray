for i in {1..20}; do
  response=$(curl -s -H "Authorization: token $GITHUB_TOKEN" \
    "https://api.github.com/repos/hcavarsan/kftray/releases")
  if echo "$response" | jq -e --arg tag "v0.2.41" '.[] | select(.tag_name == $tag and .draft == true)' > /dev/null; then
    echo "Draft release found."
    exit 0
  fi
  echo "Draft release not found. Waiting..."
  sleep 30
done
echo "Draft release not created within 10 minutes."
exit 1
