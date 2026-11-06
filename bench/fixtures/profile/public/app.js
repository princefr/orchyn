fetch('/api/profile')
  .then((res) => res.json())
  .then((profile) => {
    document.getElementById('name').textContent = profile.name;
    const tagsEl = document.getElementById('tags');
    tagsEl.append(
      ...profile.tags.map((tag) => {
        const li = document.createElement('li');
        li.textContent = tag;
        return li;
      })
    );
  });
