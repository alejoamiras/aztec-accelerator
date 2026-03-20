const observer = new IntersectionObserver(
  (entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) {
        entry.target.classList.add("revealed");
        observer.unobserve(entry.target);
      }
    }
  },
  { threshold: 0.15 },
);

for (const el of document.querySelectorAll(".reveal")) {
  observer.observe(el);
}
