import { render, screen } from "@testing-library/react";
import App from "./App";

test("renders action", () => {
  render(<App />);
  const linkElement = screen.getByText(/choose file to send/i);
  expect(linkElement).toBeInTheDocument();
});
